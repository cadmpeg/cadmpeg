// SPDX-License-Identifier: Apache-2.0
//! Datum-plane, axis, point, and coordinate-system write encoders.

use super::super::{
    format_length_like, format_point3_mm, format_vector3, is_offset_plane, require_same_family,
    valid_coordinate_frame, valid_direction, valid_plane_frame,
};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DatumPlaneReference, FeatureDefinition};

#[allow(
    clippy::unnecessary_wraps,
    reason = "Per-feature encoders use one fallible dispatch interface."
)]
impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_datum_principal_plane(
        &self,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::DatumPrincipalPlane { plane } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        let principal_planes_by_record = self.principal_planes_by_record;
        Ok({
            let record = existing.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT feature {} requires a retained principal-plane record",
                    feature.id
                ))
            })?;
            if principal_planes_by_record.get(&record.id) != Some(plane) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes its principal-plane role",
                    feature.id
                )));
            }
            (
                record.kind.clone(),
                record.parameters.clone(),
                feature.source_properties.clone(),
            )
        })
    }

    pub(super) fn encode_datum_plane_unresolved(
        &self,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::DatumPlaneUnresolved = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} has unresolved datum-plane construction",
            feature.id
        )))
    }

    pub(super) fn encode_datum_plane(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::DatumPlane {
            origin,
            normal,
            u_axis,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            if !valid_plane_frame(*normal, *u_axis) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported reference-plane semantics",
                    feature.id
                )));
            }
            if ![origin.x, origin.y, origin.z]
                .iter()
                .all(|value| value.is_finite())
            {
                return Err(CodecError::Malformed(format!(
                    "SLDPRT feature {} has a non-finite reference-plane origin",
                    feature.id
                )));
            }
            require_same_family(existing, &feature.id, &["ReferencePlane"])?;
            let mut properties = feature.source_properties.clone();
            properties.insert("Origin".into(), format_point3_mm(*origin));
            properties.insert("Normal".into(), format_vector3(*normal));
            properties.insert("UAxis".into(), format_vector3(*u_axis));
            (
                existing.map_or_else(|| "ReferencePlane".into(), |record| record.kind.clone()),
                existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            )
        })
    }

    pub(super) fn encode_datum_offset_plane(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::DatumOffsetPlane {
            reference,
            distance,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        let parent_sources = self.parent_sources;
        Ok({
            if !distance.0.is_finite() {
                return Err(CodecError::Malformed(format!(
                    "SLDPRT feature {} has a non-finite reference-plane offset",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| !is_offset_plane(record)) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes operation family",
                    feature.id
                )));
            }
            let mut properties = feature.source_properties.clone();
            match reference {
                Some(DatumPlaneReference::Feature(reference)) => {
                    let source = parent_sources.get(reference).ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing datum plane",
                            feature.id
                        ))
                    })?;
                    let key = if properties.contains_key("Plane")
                        && !properties.contains_key("Reference")
                    {
                        "Plane"
                    } else {
                        "Reference"
                    };
                    properties.insert(key.into(), source.clone());
                }
                Some(DatumPlaneReference::Face { .. }) => {
                    let Some(record) = existing else {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} cannot create a face-supported datum plane",
                            feature.id
                        )));
                    };
                    properties = record.properties.clone();
                }
                None if existing.is_some() => {}
                None => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has an unresolved datum-plane reference",
                        feature.id
                    )));
                }
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            parameters.insert(
                "D1".into(),
                format_length_like(
                    distance.0,
                    existing
                        .and_then(|record| record.parameters.get("D1"))
                        .map(String::as_str),
                ),
            );
            (
                existing.map_or_else(|| "Plane".into(), |record| record.kind.clone()),
                parameters,
                properties,
            )
        })
    }

    pub(super) fn encode_datum_axis(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::DatumAxis { origin, direction } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            if !valid_direction(*direction) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported reference-axis semantics",
                    feature.id
                )));
            }
            if ![origin.x, origin.y, origin.z]
                .iter()
                .all(|value| value.is_finite())
            {
                return Err(CodecError::Malformed(format!(
                    "SLDPRT feature {} has a non-finite reference-axis origin",
                    feature.id
                )));
            }
            require_same_family(existing, &feature.id, &["ReferenceAxis"])?;
            let mut properties = feature.source_properties.clone();
            properties.insert("Origin".into(), format_point3_mm(*origin));
            properties.insert("Direction".into(), format_vector3(*direction));
            (
                existing.map_or_else(|| "ReferenceAxis".into(), |record| record.kind.clone()),
                existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            )
        })
    }

    pub(super) fn encode_datum_point(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::DatumPoint { position, .. } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            if ![position.x, position.y, position.z]
                .iter()
                .all(|value| value.is_finite())
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported reference-point semantics",
                    feature.id
                )));
            }
            require_same_family(existing, &feature.id, &["ReferencePoint"])?;
            let mut properties = feature.source_properties.clone();
            properties.insert("Position".into(), format_point3_mm(*position));
            (
                existing.map_or_else(|| "ReferencePoint".into(), |record| record.kind.clone()),
                existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            )
        })
    }

    pub(super) fn encode_datum_coordinate_system(
        &self,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::DatumCoordinateSystem {
            origin,
            x_axis,
            y_axis,
            z_axis,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            if !valid_coordinate_frame(*origin, *x_axis, *y_axis, *z_axis) {
                return Err(CodecError::Malformed(format!(
                    "SLDPRT feature {} has an invalid coordinate-system frame",
                    feature.id
                )));
            }
            require_same_family(
                existing,
                &feature.id,
                &["CoordinateSystem", "ReferenceCoordinateSystem"],
            )?;
            let mut properties = feature.source_properties.clone();
            properties.insert("Origin".into(), format_point3_mm(*origin));
            properties.insert("XAxis".into(), format_vector3(*x_axis));
            properties.insert("YAxis".into(), format_vector3(*y_axis));
            properties.insert("ZAxis".into(), format_vector3(*z_axis));
            (
                existing.map_or_else(|| "CoordinateSystem".into(), |record| record.kind.clone()),
                existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            )
        })
    }
}
