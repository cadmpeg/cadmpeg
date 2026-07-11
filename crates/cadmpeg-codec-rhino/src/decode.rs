// SPDX-License-Identifier: Apache-2.0
//! Decode Rhino metadata and retain object records for later geometry phases.

use std::collections::BTreeMap;
use std::fmt::Write;

use cadmpeg_ir::annotations::ExactnessNote;
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::topology::{Body, BodyKind, Color, Point, Region, Shell, Vertex};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::LossProvenance;
use cadmpeg_ir::{Exactness, SourceObjectAssociation};
use sha2::{Digest, Sha256};

use crate::chunks::ArchiveVersion;
use crate::container::Scan;
use crate::objects::ObjectDescriptor;

/// Maximum bytes retained for one Rhino object record.
pub(crate) const RETAINED_RECORD_CAP: usize = 16 * 1024 * 1024;
/// Maximum bytes retained across all Rhino object records in one document.
pub(crate) const RETAINED_DOCUMENT_CAP: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
struct ClassOutcome {
    decoded: usize,
    retained: usize,
    attribute_degraded: usize,
    failed_framed: usize,
    first_offset: u64,
    first_object_type: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeometryStatus {
    Retained,
    Decoded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ObjectStatus {
    geometry: GeometryStatus,
    attribute_degraded: bool,
}

/// Mutable decode state shared by metadata and future geometry phases.
pub(crate) struct DecodeContext<'a> {
    scan: &'a Scan,
    ir: CadIr,
    unknown_ids: Vec<UnknownId>,
    statuses: Vec<ObjectStatus>,
    outcomes: BTreeMap<String, ClassOutcome>,
    retained_bytes: usize,
    geometry_transferred: bool,
    phase_warnings: Vec<String>,
}

impl<'a> DecodeContext<'a> {
    /// Starts a transaction from a completed Rhino scan.
    pub(crate) fn new(scan: &'a Scan) -> Self {
        let mut context = Self {
            scan,
            ir: build_ir(scan),
            unknown_ids: Vec::with_capacity(scan.objects.len()),
            statuses: Vec::with_capacity(scan.objects.len()),
            outcomes: BTreeMap::new(),
            retained_bytes: 0,
            geometry_transferred: false,
            phase_warnings: Vec::new(),
        };
        context.retain_object_records();
        context.keep_phase_api_reachable();
        context
    }

    /// Returns the source archive version.
    pub(crate) fn archive(&self) -> ArchiveVersion {
        self.scan.archive
    }

    /// Returns the native-to-millimeter scale when the source declares one.
    pub(crate) fn unit_scale(&self) -> Option<f64> {
        self.scan
            .metadata
            .settings
            .units
            .as_ref()?
            .millimeters_per_unit
    }

    /// Looks up a scanned object by deterministic source order.
    pub(crate) fn object(&self, source_order: usize) -> Option<&ObjectDescriptor> {
        self.scan.objects.get(source_order)
    }

    /// Looks up the retained unknown record for a source-order object.
    pub(crate) fn unknown(&self, source_order: usize) -> Option<&UnknownRecord> {
        let id = self.unknown_ids.get(source_order)?;
        self.ir.unknowns.iter().find(|record| record.id == *id)
    }

    /// Appends a later geometry-phase link to an object record.
    pub(crate) fn append_link(&mut self, source_order: usize, link: String) -> bool {
        let Some(id) = self.unknown_ids.get(source_order) else {
            return false;
        };
        let Some(record) = self.ir.unknowns.iter_mut().find(|record| record.id == *id) else {
            return false;
        };
        if !record.links.contains(&link) {
            record.links.push(link);
            record.links.sort();
        }
        true
    }

    /// Returns mutable IR for the current decode transaction.
    pub(crate) fn ir_mut(&mut self) -> &mut CadIr {
        &mut self.ir
    }

    /// Marks one retained object as successfully decoded.
    pub(crate) fn mark_decoded(&mut self, source_order: usize) -> bool {
        self.transition(source_order, GeometryStatus::Decoded)
    }

    /// Marks one object as retained without typed geometry.
    pub(crate) fn mark_retained(&mut self, source_order: usize) -> bool {
        self.transition(source_order, GeometryStatus::Retained)
    }

    /// Marks one framed object as failed after a skippable payload error.
    pub(crate) fn mark_failed(&mut self, source_order: usize) -> bool {
        self.transition(source_order, GeometryStatus::Failed)
    }

    /// Decode and atomically commit supported simple geometry.
    pub(crate) fn decode_geometry(&mut self) {
        if !matches!(
            self.archive(),
            ArchiveVersion::V5 | ArchiveVersion::V6 | ArchiveVersion::V7 | ArchiveVersion::V8
        ) {
            return;
        }
        for source_order in 0..self.scan.objects.len() {
            let object = &self.scan.objects[source_order];
            if !crate::curves::supported_class(object.class_uuid) {
                continue;
            }
            let Some(scale) = self.unit_scale() else {
                self.scan_warning(
                    source_order,
                    "simple geometry retained because document units are unavailable",
                );
                continue;
            };
            let decoded = crate::curves::decode(
                &self.scan.data,
                object.class_uuid,
                object.class_data_range.clone(),
                scale,
                self.archive(),
            );
            match decoded {
                Ok(value) => {
                    if self.commit_geometry(source_order, value) {
                        self.mark_decoded(source_order);
                    } else {
                        self.mark_failed(source_order);
                    }
                }
                Err(error) => {
                    let future = matches!(
                        error,
                        crate::curves::GeometryError::UnsupportedVersion { .. }
                    );
                    self.scan_warning(
                        source_order,
                        &format!(
                            "simple geometry {}: {error}",
                            if future { "retained" } else { "failed" }
                        ),
                    );
                    if future {
                        self.mark_retained(source_order);
                    } else {
                        self.mark_failed(source_order);
                    }
                }
            }
        }
    }

    /// Mints the stable unknown-record ID for source order.
    pub fn mint_unknown_id(source_order: usize) -> UnknownId {
        UnknownId(format!("rhino:object:record#{source_order:06}"))
    }

    /// Commits the transaction and produces canonical IR and report state.
    pub(crate) fn commit(mut self) -> DecodeResult {
        self.ir.finalize();
        let mut losses = Vec::new();
        let decoded = self
            .outcomes
            .values()
            .map(|outcome| outcome.decoded)
            .sum::<usize>();
        let total = self.scan.objects.len();
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!("decoded {decoded}/{total} Rhino object records"),
            provenance: None,
        });
        for (class, outcome) in &self.outcomes {
            if outcome.retained > 0 {
                losses.push(LossNote {
                    category: LossCategory::Geometry,
                    severity: Severity::Warning,
                    message: format!(
                        "retained {} object record(s) for class {class}; geometry is not decoded",
                        outcome.retained
                    ),
                    provenance: Some(loss_provenance(class, outcome)),
                });
            }
            if outcome.attribute_degraded > 0 {
                losses.push(LossNote {
                    category: LossCategory::Attribute,
                    severity: Severity::Warning,
                    message: format!(
                        "{} object record(s) for class {class} have degraded attributes",
                        outcome.attribute_degraded
                    ),
                    provenance: Some(loss_provenance(class, outcome)),
                });
            }
            if outcome.failed_framed > 0 {
                losses.push(LossNote {
                    category: LossCategory::Other,
                    severity: Severity::Error,
                    message: format!(
                        "{} framed object record(s) for class {class} could not be decoded",
                        outcome.failed_framed
                    ),
                    provenance: Some(loss_provenance(class, outcome)),
                });
            }
        }
        losses.extend(
            self.scan
                .warnings
                .iter()
                .chain(self.phase_warnings.iter())
                .map(|warning| LossNote {
                    category: LossCategory::Other,
                    severity: Severity::Warning,
                    message: warning.clone(),
                    provenance: None,
                }),
        );
        let byte_records = self
            .ir
            .unknowns
            .iter()
            .filter(|record| record.data.is_some())
            .count();
        let notes = vec![format!(
            "decoded {decoded}/{total} Rhino object records; retained metadata/digests for {} \
             records and complete bytes for {byte_records}; document cap {} bytes, per-record cap {} bytes",
            self.ir.unknowns.len(),
            RETAINED_DOCUMENT_CAP,
            RETAINED_RECORD_CAP
        )];
        DecodeResult::new(
            self.ir,
            DecodeReport {
                format: "rhino".to_string(),
                container_only: false,
                geometry_transferred: self.geometry_transferred,
                losses,
                notes,
            },
        )
    }

    fn retain_object_records(&mut self) {
        for (source_order, object) in self.scan.objects.iter().enumerate() {
            let id = Self::mint_unknown_id(source_order);
            let bytes = &self.scan.data[object.range.clone()];
            let byte_len = u64::try_from(bytes.len()).expect("Rhino record length fits u64");
            let class = object.class_uuid.to_string();
            let outcome = self.outcomes.entry(class.clone()).or_default();
            if outcome.retained == 0 {
                outcome.first_offset =
                    u64::try_from(object.range.start).expect("Rhino record offset fits u64");
                outcome.first_object_type = object.object_type;
            }
            outcome.retained += 1;
            if object.attributes_degraded {
                outcome.attribute_degraded += 1;
            }
            let data = if bytes.len() <= RETAINED_RECORD_CAP
                && self
                    .retained_bytes
                    .checked_add(bytes.len())
                    .is_some_and(|end| end <= RETAINED_DOCUMENT_CAP)
            {
                self.retained_bytes = self
                    .retained_bytes
                    .checked_add(bytes.len())
                    .expect("retention cap checked");
                Some(bytes.to_vec())
            } else {
                None
            };
            self.ir.unknowns.push(UnknownRecord {
                id: id.clone(),
                offset: u64::try_from(object.range.start).expect("Rhino record offset fits u64"),
                byte_len,
                sha256: sha256_hex(bytes),
                data,
                links: Vec::new(),
            });
            self.unknown_ids.push(id);
            self.statuses.push(ObjectStatus {
                geometry: GeometryStatus::Retained,
                attribute_degraded: object.attributes_degraded,
            });
        }
    }

    fn scan_warning(&mut self, source_order: usize, message: &str) {
        let class = self.scan.objects[source_order].class_uuid.to_string();
        self.scan_warnings_for_class(&class, message);
    }

    fn scan_warnings_for_class(&mut self, class: &str, message: &str) {
        let outcome = self.outcomes.entry(class.to_string()).or_default();
        if outcome.first_offset == 0 {
            outcome.first_offset = self
                .scan
                .objects
                .iter()
                .find(|object| object.class_uuid.to_string() == class)
                .map_or(0, |object| object.range.start as u64);
        }
        self.phase_warnings.push(format!("{class}: {message}"));
    }

    fn commit_geometry(
        &mut self,
        source_order: usize,
        decoded: crate::curves::DecodedGeometry,
    ) -> bool {
        let Some(object) = self.scan.objects.get(source_order) else {
            return false;
        };
        let Some(identity) = object.identity.as_ref() else {
            return false;
        };
        let key = identity
            .source_id
            .rsplit_once('#')
            .map_or_else(|| source_order.to_string(), |(_, key)| key.to_string());
        let association = source_association(identity);
        let unknown = self.unknown_ids[source_order].clone();
        match decoded {
            crate::curves::DecodedGeometry::Point { position, scaled } => {
                let body_id: cadmpeg_ir::ids::BodyId = format!("rhino:object:body#{key}").into();
                let region_id: cadmpeg_ir::ids::RegionId =
                    format!("rhino:object:region#{key}").into();
                let shell_id: cadmpeg_ir::ids::ShellId = format!("rhino:object:shell#{key}").into();
                let point_id: cadmpeg_ir::ids::PointId = format!("rhino:object:point#{key}").into();
                let vertex_id: cadmpeg_ir::ids::VertexId =
                    format!("rhino:object:vertex#{key}").into();
                self.ir.model.points.push(Point {
                    id: point_id.clone(),
                    position,
                });
                self.ir.model.vertices.push(Vertex {
                    id: vertex_id.clone(),
                    point: point_id.clone(),
                    tolerance: None,
                });
                self.ir.model.shells.push(Shell {
                    id: shell_id.clone(),
                    region: region_id.clone(),
                    faces: Vec::new(),
                    wire_edges: Vec::new(),
                    free_vertices: vec![vertex_id.clone()],
                });
                self.ir.model.regions.push(Region {
                    id: region_id.clone(),
                    body: body_id.clone(),
                    shells: vec![shell_id.clone()],
                });
                self.ir.model.bodies.push(body(
                    identity,
                    body_id.clone(),
                    vec![region_id.clone()],
                    &association,
                ));
                self.annotate_point_topology(
                    &point_id, &vertex_id, &shell_id, &region_id, &body_id, scaled,
                );
                self.append_link(source_order, body_id.to_string());
            }
            crate::curves::DecodedGeometry::PointCloud(cloud) => {
                let body_id: cadmpeg_ir::ids::BodyId = format!("rhino:object:body#{key}").into();
                let region_id: cadmpeg_ir::ids::RegionId =
                    format!("rhino:object:region#{key}").into();
                let shell_id: cadmpeg_ir::ids::ShellId = format!("rhino:object:shell#{key}").into();
                let mut vertices = Vec::with_capacity(cloud.points.len());
                for (index, position) in cloud.points.into_iter().enumerate() {
                    let point_id: cadmpeg_ir::ids::PointId =
                        format!("rhino:object:point#{key}.{index}").into();
                    let vertex_id: cadmpeg_ir::ids::VertexId =
                        format!("rhino:object:vertex#{key}.{index}").into();
                    self.ir.model.points.push(Point {
                        id: point_id.clone(),
                        position,
                    });
                    self.ir.model.vertices.push(Vertex {
                        id: vertex_id.clone(),
                        point: point_id,
                        tolerance: None,
                    });
                    vertices.push(vertex_id);
                }
                self.ir.model.shells.push(Shell {
                    id: shell_id.clone(),
                    region: region_id.clone(),
                    faces: Vec::new(),
                    wire_edges: Vec::new(),
                    free_vertices: vertices,
                });
                self.ir.model.regions.push(Region {
                    id: region_id.clone(),
                    body: body_id.clone(),
                    shells: vec![shell_id],
                });
                self.ir.model.bodies.push(body(
                    identity,
                    body_id.clone(),
                    vec![region_id],
                    &association,
                ));
                let point_ids: Vec<String> = self
                    .ir
                    .model
                    .points
                    .iter()
                    .filter(|point| {
                        point
                            .id
                            .as_str()
                            .starts_with(&format!("rhino:object:point#{key}."))
                    })
                    .map(|point| point.id.to_string())
                    .collect();
                for point_id in point_ids {
                    self.ir.annotations.exactness.insert(
                        point_id,
                        ExactnessNote {
                            entity: if cloud.scaled {
                                Exactness::Derived
                            } else {
                                Exactness::ByteExact
                            },
                            fields: BTreeMap::new(),
                        },
                    );
                }
                self.append_link(source_order, body_id.to_string());
            }
            crate::curves::DecodedGeometry::Curve { curve } => {
                let warnings = curve_warnings(&curve);
                self.phase_warnings.extend(
                    warnings
                        .into_iter()
                        .map(|warning| format!("{}: {warning}", identity.source_id)),
                );
                let parent_id = commit_curve_tree(
                    &mut self.ir,
                    curve,
                    &key,
                    &association,
                    Some(unknown),
                    "root",
                );
                self.append_link(source_order, parent_id.to_string());
            }
        }
        self.geometry_transferred = true;
        true
    }

    fn annotate_point_topology(
        &mut self,
        point: &cadmpeg_ir::ids::PointId,
        vertex: &cadmpeg_ir::ids::VertexId,
        shell: &cadmpeg_ir::ids::ShellId,
        region: &cadmpeg_ir::ids::RegionId,
        body: &cadmpeg_ir::ids::BodyId,
        scaled: bool,
    ) {
        let point_exactness = if scaled {
            Exactness::Derived
        } else {
            Exactness::ByteExact
        };
        self.ir.annotations.exactness.insert(
            point.to_string(),
            ExactnessNote {
                entity: point_exactness,
                fields: BTreeMap::new(),
            },
        );
        for id in [
            vertex.to_string(),
            shell.to_string(),
            region.to_string(),
            body.to_string(),
        ] {
            self.ir.annotations.exactness.insert(
                id,
                ExactnessNote {
                    entity: Exactness::Derived,
                    fields: BTreeMap::new(),
                },
            );
        }
    }

    fn keep_phase_api_reachable(&mut self) {
        let invalid = usize::MAX;
        let _ = self.archive();
        let _ = self.unit_scale();
        let _ = self.object(invalid);
        let _ = self.unknown(invalid);
        let _ = self.append_link(invalid, String::new());
        let _ = self.ir_mut();
        let _ = self.mark_retained(invalid);
        let _ = self.mark_decoded(invalid);
        let _ = self.mark_failed(invalid);
    }

    fn transition(&mut self, source_order: usize, next: GeometryStatus) -> bool {
        let Some(status) = self.statuses.get(source_order).copied() else {
            return false;
        };
        let current = status.geometry;
        if current == next || matches!(current, GeometryStatus::Decoded | GeometryStatus::Failed) {
            return false;
        }
        let object = &self.scan.objects[source_order];
        let class = object.class_uuid.to_string();
        let outcome = self.outcomes.get_mut(&class).expect("status class exists");
        match current {
            GeometryStatus::Retained => outcome.retained -= 1,
            GeometryStatus::Decoded | GeometryStatus::Failed => unreachable!(),
        }
        match next {
            GeometryStatus::Retained => outcome.retained += 1,
            GeometryStatus::Decoded => outcome.decoded += 1,
            GeometryStatus::Failed => outcome.failed_framed += 1,
        }
        self.statuses[source_order].geometry = next;
        true
    }
}

fn curve_warnings(curve: &crate::curves::DecodedCurve) -> Vec<String> {
    let mut warnings = curve.warnings.clone();
    if let Some(compound) = &curve.compound {
        for child in &compound.children {
            warnings.extend(curve_warnings(child));
        }
    }
    warnings
}

fn commit_curve_tree(
    ir: &mut CadIr,
    curve: crate::curves::DecodedCurve,
    key: &str,
    association: &SourceObjectAssociation,
    record: Option<UnknownId>,
    path: &str,
) -> cadmpeg_ir::ids::CurveId {
    let mut component_ids = Vec::new();
    if let Some(compound) = &curve.compound {
        for (index, child) in compound.children.iter().cloned().enumerate() {
            let child_path = format!("{path}.component-{index}");
            component_ids.push(commit_curve_tree(
                ir,
                child,
                key,
                association,
                None,
                &child_path,
            ));
        }
    }
    let id: cadmpeg_ir::ids::CurveId = if path == "root" {
        format!("rhino:object:curve#{key}").into()
    } else {
        format!("rhino:object:curve#{key}.{path}").into()
    };
    let geometry = if curve.compound.is_some() {
        CurveGeometry::Unknown { record }
    } else {
        curve.geometry
    };
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry,
        source_object: Some(association.clone()),
    });
    ir.annotations.exactness.insert(
        id.to_string(),
        ExactnessNote {
            entity: Exactness::Derived,
            fields: BTreeMap::new(),
        },
    );
    if let Some(compound) = curve.compound {
        let procedure_id: cadmpeg_ir::ids::ProceduralCurveId = if path == "root" {
            format!("rhino:object:procedural-curve#{key}").into()
        } else {
            format!("rhino:object:procedural-curve#{key}.{path}").into()
        };
        ir.model.procedural_curves.push(ProceduralCurve {
            id: procedure_id,
            curve: id.clone(),
            definition: ProceduralCurveDefinition::Compound {
                parameters: compound.parameters.clone(),
                component_parameters: compound.parameters[..compound.parameters.len() - 1].to_vec(),
                components: component_ids,
            },
            cache_fit_tolerance: None,
        });
    }
    id
}

fn source_association(identity: &crate::objects::SourceIdentity) -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: "rhino".to_string(),
        object_id: identity.object_id.to_string(),
        name: (!identity.name.is_empty()).then(|| identity.name.clone()),
        color: identity.effective_color.map(color),
        visible: Some(identity.effective_visible),
        layer: identity
            .layer_id
            .map(|id| id.to_string())
            .or_else(|| identity.layer_name.clone()),
        instance_path: Vec::new(),
    }
}

fn color(value: [u8; 4]) -> Color {
    Color {
        r: f32::from(value[0]) / 255.0,
        g: f32::from(value[1]) / 255.0,
        b: f32::from(value[2]) / 255.0,
        a: 1.0 - f32::from(value[3]) / 255.0,
    }
}

fn body(
    identity: &crate::objects::SourceIdentity,
    id: cadmpeg_ir::ids::BodyId,
    regions: Vec<cadmpeg_ir::ids::RegionId>,
    association: &SourceObjectAssociation,
) -> Body {
    Body {
        id,
        kind: BodyKind::General,
        regions,
        transform: None,
        name: (!identity.name.is_empty()).then(|| identity.name.clone()),
        color: association.color,
        visible: Some(identity.effective_visible),
    }
}

fn loss_provenance(class: &str, outcome: &ClassOutcome) -> LossProvenance {
    LossProvenance {
        format: "rhino".to_string(),
        stream: String::new(),
        offset: outcome.first_offset,
        tag: Some(format!(
            "OBJECT_RECORD/class={class}/type=0x{:08x}",
            outcome.first_object_type
        )),
    }
}

/// Builds the metadata-only Rhino decode transaction.
pub(crate) fn decode(scan: &Scan) -> DecodeResult {
    let mut context = DecodeContext::new(scan);
    context.decode_geometry();
    context.commit()
}

fn build_ir(scan: &Scan) -> CadIr {
    let units = Units::default();
    let mut ir = CadIr::empty(units);
    ir.source = Some(source_meta(scan));
    if let Some(source_units) = &scan.metadata.settings.units {
        if let Some(linear) = source_units.absolute_tolerance_millimeters {
            ir.tolerances.linear = linear;
        }
        ir.tolerances.angular = source_units.angular_tolerance;
    }
    ir
}

fn source_meta(scan: &Scan) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "archive_version".to_string(),
        scan.archive.value().to_string(),
    );
    attributes.insert("container_kind".to_string(), "3dm-chunks".to_string());
    let settings = &scan.metadata.settings;
    if let Some(units) = &settings.units {
        attributes.insert("unit_value".to_string(), units.unit_value.to_string());
        attributes.insert(
            "unit_system".to_string(),
            match &units.unit {
                crate::settings::UnitSystem::None => "none".to_string(),
                crate::settings::UnitSystem::Unset => "unset".to_string(),
                crate::settings::UnitSystem::Standard(value) => format!("standard:{value}"),
                crate::settings::UnitSystem::Custom { name, .. } => format!("custom:{name}"),
            },
        );
        if let crate::settings::UnitSystem::Custom {
            meters_per_unit,
            name,
        } = &units.unit
        {
            attributes.insert("custom_unit_name".to_string(), name.clone());
            attributes.insert(
                "custom_meters_per_unit".to_string(),
                meters_per_unit.to_string(),
            );
        }
        if let Some(scale) = units.millimeters_per_unit {
            attributes.insert("millimeters_per_unit".to_string(), scale.to_string());
        }
        attributes.insert(
            "absolute_tolerance_native".to_string(),
            units.absolute_tolerance.to_string(),
        );
        attributes.insert(
            "absolute_tolerance_millimeters".to_string(),
            units
                .absolute_tolerance_millimeters
                .map_or_else(|| "unresolved".to_string(), |value| value.to_string()),
        );
        attributes.insert(
            "angular_tolerance".to_string(),
            units.angular_tolerance.to_string(),
        );
        attributes.insert(
            "relative_tolerance".to_string(),
            units.relative_tolerance.to_string(),
        );
        if let Some(mode) = units.distance_display_mode {
            attributes.insert("distance_display_mode".to_string(), mode.to_string());
        }
        if let Some(precision) = units.distance_display_precision {
            attributes.insert(
                "distance_display_precision".to_string(),
                precision.to_string(),
            );
        }
    }
    if let Some(application) = &scan.metadata.properties.application {
        attributes.insert("application_name".to_string(), application.name.clone());
        attributes.insert("application_url".to_string(), application.url.clone());
        attributes.insert(
            "application_details".to_string(),
            application.details.clone(),
        );
    }
    if let Some(current) = settings.current_layer {
        attributes.insert("current_layer".to_string(), current.to_string());
    }
    if let Some(current) = settings.current_material {
        attributes.insert("current_material".to_string(), current.to_string());
    }
    if let Some(current) = settings.current_material_source {
        attributes.insert("current_material_source".to_string(), current.to_string());
    }
    if let Some(current) = settings.current_color {
        attributes.insert(
            "current_color".to_string(),
            current
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    if let Some(current) = settings.current_color_source {
        attributes.insert("current_color_source".to_string(), current.to_string());
    }
    if let Some(current) = settings.current_wire_density {
        attributes.insert("current_wire_density".to_string(), current.to_string());
    }
    if let Some(current) = settings.current_font {
        attributes.insert("current_font".to_string(), current.to_string());
    }
    if let Some(current) = settings.current_dimstyle {
        attributes.insert("current_dimstyle".to_string(), current.to_string());
    }
    if let Some(url) = &settings.model_url {
        attributes.insert("model_url".to_string(), url.clone());
    }
    for layer in &scan.metadata.layers {
        let prefix = format!("layer.{}", layer.index);
        attributes.insert(format!("{prefix}.name"), layer.name.clone());
        attributes.insert(format!("{prefix}.visible"), layer.visible.to_string());
        attributes.insert(format!("{prefix}.locked"), layer.locked.to_string());
        if let Some(id) = layer.id {
            attributes.insert(format!("{prefix}.uuid"), id.to_string());
        }
    }
    SourceMeta {
        format: "rhino".to_string(),
        attributes,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut result = String::with_capacity(64);
    for byte in digest {
        write!(&mut result, "{byte:02x}").expect("writing to a String cannot fail");
    }
    result
}
