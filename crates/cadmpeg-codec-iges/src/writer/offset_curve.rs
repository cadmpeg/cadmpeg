// SPDX-License-Identifier: Apache-2.0
//! Write exact curve offsets.

use super::{
    curve_reference_span, finite, is_support_curve, number, reference_marker, CodecError,
    CurveEntityEmitter, CurveGeometry, CurveId, CurveSpan, Entity, EntityStatus, Sense,
    SolvedCurveGeometry, Vector3, TAU,
};

impl CurveEntityEmitter<'_> {
    pub(super) fn append_offset(
        &mut self,
        curve_id: &CurveId,
        geometry: &CurveGeometry,
        span: Option<&CurveSpan>,
        status: EntityStatus,
    ) -> Result<Option<usize>, CodecError> {
        if let Some(index) = self.append_linear_line_offset(curve_id, geometry, span, status)? {
            return Ok(Some(index));
        }
        self.append_uniform_circle_offset(curve_id, geometry, span, status)
    }

    pub(super) fn append_linear_line_offset(
        &mut self,
        curve_id: &CurveId,
        geometry: &CurveGeometry,
        span: Option<&CurveSpan>,
        status: EntityStatus,
    ) -> Result<Option<usize>, CodecError> {
        use cadmpeg_ir::geometry::{
            nurbs::{NurbsCurve, NurbsPoles3},
            CurveOffsetDistanceLaw, CurveOffsetLawBasis, CurveOffsetRange, OffsetSide,
            ProceduralCurveDefinition,
        };
        let Some(procedural) = self
            .ir
            .model
            .procedural_curves
            .iter()
            .find(|record| self.ir.model.procedural_curve_owner(&record.id) == Some(curve_id))
        else {
            return Ok(None);
        };
        let ProceduralCurveDefinition::Offset(offset) = procedural.definition() else {
            return Ok(None);
        };
        let (
            Some(CurveOffsetRange::Variable {
                parameter_range,
                distance_law:
                    CurveOffsetDistanceLaw::Linear {
                        basis,
                        distances,
                        control_range,
                    },
            }),
            OffsetSide::PlaneNormal { normal },
        ) = (offset.range(), offset.side())
        else {
            return Ok(None);
        };
        let Some(source) = self
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *offset.source())
        else {
            return Ok(None);
        };
        let (
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line)),
            Some(SolvedCurveGeometry::Nurbs(current)),
            Some(span),
        ) = (&source.geometry, geometry.solved(), span)
        else {
            return Ok(None);
        };
        if !is_support_curve(source) {
            return Ok(None);
        }
        let source_span = curve_reference_span(self.ir, &source.id, &source.geometry)?;
        let range = parameter_range.endpoints();
        let length = source_span.end.distance(source_span.start);
        let direction = *line.direction().as_raw();
        if range[0] != 0.0
            || range[1] != length
            || source_span.range.get() != range
            || span.range.get() != range
            || normal.get().dot(direction) != 0.0
        {
            return Ok(None);
        }
        let control = control_range.endpoints();
        let distances = [distances[0].get(), distances[1].get()];
        let law_at = |parameter: f64| {
            let independent = match basis {
                CurveOffsetLawBasis::ArcLength => parameter - range[0],
                CurveOffsetLawBasis::Parameter => parameter,
            };
            distances[0]
                + (independent - control[0]) / (control[1] - control[0])
                    * (distances[1] - distances[0])
        };
        let side = normal.get().cross(direction);
        let points = vec![
            source_span.start.translated(side, law_at(range[0])),
            source_span.end.translated(side, law_at(range[1])),
        ];
        let Ok(expected_curve) = NurbsCurve::new(
            1,
            vec![range[0], range[0], range[1], range[1]],
            NurbsPoles3::Polynomial { points },
            false,
        ) else {
            return Ok(None);
        };
        let Ok(expected_offset) =
            cadmpeg_ir::geometry::curve_payloads::OffsetCurveConstruction::try_new(
                source.id.clone(),
                distances[0],
                OffsetSide::PlaneNormal {
                    normal: normal.get(),
                },
                offset.range().as_ref().map(CurveOffsetRange::to_raw),
            )
        else {
            return Ok(None);
        };
        if &expected_curve != current || &expected_offset != offset {
            return Ok(None);
        }
        let (basis_code, controls) = match basis {
            CurveOffsetLawBasis::ArcLength => (1, control),
            CurveOffsetLawBasis::Parameter => (2, control.map(|value| value / length)),
        };
        let [Some(d1), Some(c1), Some(d2), Some(c2), Some(nx), Some(ny), Some(nz)] = [
            distances[0],
            controls[0],
            distances[1],
            controls[1],
            normal.get().x,
            normal.get().y,
            normal.get().z,
        ]
        .map(cadmpeg_ir::scalar::FiniteReal::new) else {
            return Ok(None);
        };
        let source_index = if let Some(index) = self.entities.iter().position(|entity| {
            entity.type_code == 110
                && entity.status.is_physically_dependent()
                && entity
                    .source_targets
                    .iter()
                    .any(|id| id == source.id.as_str())
        }) {
            index
        } else {
            self.append(
                &source.id,
                &source.geometry,
                Some(&source_span),
                Sense::Forward,
                EntityStatus::PhysicallyDependent,
            )?
        };
        let Some(pointer) = source_index.checked_add(self.reference_offset) else {
            return Ok(None);
        };
        let mut entity = Entity {
            structure: None,
            property_links: Vec::new(),
            association_links: Vec::new(),
            line_font: None,
            source_targets: Vec::new(),
            type_code: 130,
            form: 0,
            label: String::new(),
            status,
            subscript: 0,
            parameter_body: format!(
                "{},2,0,0,{},{},{},{},{},{},{},{},0,1;",
                reference_marker(pointer),
                basis_code,
                number(d1),
                number(c1),
                number(d2),
                number(c2),
                number(nx),
                number(ny),
                number(nz)
            )
            .into_bytes(),
            transform: None,
            transform_link: None,
        };
        if let Some(sequence) = self
            .ir
            .model
            .curves
            .iter()
            .find(|curve| &curve.id == curve_id)
            .and_then(|curve| curve.source_object.as_ref())
            .filter(|source| source.format == cadmpeg_ir::CodecFormat::Iges)
            .and_then(|source| source.object_id.as_str().strip_prefix('D'))
        {
            super::native_records::numeric::retain_directory(
                self.ir,
                &format!("iges:entity:directory#{sequence}"),
                &mut entity,
            );
        }
        let index = self.entities.len();
        self.entities.push(entity);
        Ok(Some(index))
    }

    pub(super) fn append_uniform_circle_offset(
        &mut self,
        curve_id: &CurveId,
        geometry: &CurveGeometry,
        span: Option<&CurveSpan>,
        status: EntityStatus,
    ) -> Result<Option<usize>, CodecError> {
        use cadmpeg_ir::geometry::{CurveOffsetRange, OffsetSide, ProceduralCurveDefinition};
        let Some(procedural) = self
            .ir
            .model
            .procedural_curves
            .iter()
            .find(|record| self.ir.model.procedural_curve_owner(&record.id) == Some(curve_id))
        else {
            return Ok(None);
        };
        let ProceduralCurveDefinition::Offset(offset) = procedural.definition() else {
            return Ok(None);
        };
        let (
            Some(CurveOffsetRange::Uniform { parameter_range }),
            OffsetSide::PlaneNormal { normal },
        ) = (offset.range(), offset.side())
        else {
            return Ok(None);
        };
        let Some(span) = span else {
            return Ok(None);
        };
        let range = parameter_range.endpoints();
        if span.range.get() != range
            || range[0] != 0.0
            || range[1] > TAU
            || normal.get() != Vector3::new(0.0, 0.0, 1.0)
        {
            return Ok(None);
        }
        let Some(source) = self
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *offset.source())
        else {
            return Ok(None);
        };
        if !is_support_curve(source) {
            return Ok(None);
        }
        let (
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(base)),
            Some(SolvedCurveGeometry::Circle(result)),
        ) = (&source.geometry, geometry.solved())
        else {
            return Ok(None);
        };
        if source
            .parameter_range
            .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
            != Some(range)
            || base.center() != result.center()
            || base.frame() != result.frame()
            || *base.frame().axis().as_raw() != Vector3::new(0.0, 0.0, 1.0)
            || *base.frame().reference().as_raw() != Vector3::new(1.0, 0.0, 0.0)
        {
            return Ok(None);
        }
        let distance = base.radius().get() - result.radius().get();
        let Ok(uniform_range) = CurveOffsetRange::uniform(range) else {
            return Ok(None);
        };
        let Ok(expected) = cadmpeg_ir::geometry::curve_payloads::OffsetCurveConstruction::try_new(
            source.id.clone(),
            distance,
            OffsetSide::PlaneNormal {
                normal: normal.get(),
            },
            Some(uniform_range),
        ) else {
            return Ok(None);
        };
        if &expected != offset {
            return Ok(None);
        }
        let source_span = curve_reference_span(self.ir, &source.id, &source.geometry)?;
        let source_index = if let Some(index) = self.entities.iter().position(|entity| {
            entity.type_code == 100
                && entity
                    .source_targets
                    .iter()
                    .any(|id| id == source.id.as_str())
                && entity.status.is_physically_dependent()
        }) {
            index
        } else {
            self.append(
                &source.id,
                &source.geometry,
                Some(&source_span),
                Sense::Forward,
                EntityStatus::PhysicallyDependent,
            )?
        };
        let pointer = source_index
            .checked_add(self.reference_offset)
            .ok_or_else(|| CodecError::malformed("IGES offset source index overflows"))?;
        let index = self.entities.len();
        let mut entity = Entity {
            structure: None,
            property_links: Vec::new(),
            association_links: Vec::new(),
            line_font: None,
            source_targets: Vec::new(),
            type_code: 130,
            form: 0,
            label: String::new(),
            status,
            subscript: 0,
            parameter_body: format!(
                "{},1,0,0,0,{},0,0,0,0,0,1,{},{};",
                reference_marker(pointer),
                number(finite(distance, "offset distance")?),
                number(finite(range[0], "offset start")?),
                number(finite(range[1], "offset end")?)
            )
            .into_bytes(),
            transform: None,
            transform_link: None,
        };
        if let Some(sequence) = self
            .ir
            .model
            .curves
            .iter()
            .find(|curve| &curve.id == curve_id)
            .and_then(|curve| curve.source_object.as_ref())
            .filter(|source| source.format == cadmpeg_ir::CodecFormat::Iges)
            .and_then(|source| source.object_id.as_str().strip_prefix('D'))
        {
            super::native_records::numeric::retain_directory(
                self.ir,
                &format!("iges:entity:directory#{sequence}"),
                &mut entity,
            );
        }
        self.entities.push(entity);
        Ok(Some(index))
    }
}
