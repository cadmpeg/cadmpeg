// SPDX-License-Identifier: Apache-2.0
//! Registered list values held by their actual archive entry.

use super::PropertyReader;
use crate::native::entry_contents::{EntryContents, F32Values, FilletValue};
use crate::native::{EntryRecord, PropertyRecord};
use cadmpeg_ir::scalar::{FiniteBinary32, FiniteReal};

pub(crate) struct MeshMaterialSlots<'a> {
    pub(crate) binding: Option<u32>,
    pub(crate) colors: [Option<&'a [Option<u32>]>; 4],
    pub(crate) shininess: Option<&'a [Option<FiniteBinary32>]>,
    pub(crate) transparency: Option<&'a [Option<FiniteBinary32>]>,
}

impl<'ctx, 'arena> PropertyReader<'ctx, 'arena> {
    pub(crate) fn with_entries(
        mut self,
        entries: impl Into<std::borrow::Cow<'ctx, [EntryRecord]>>,
    ) -> Self {
        self.entries = entries.into();
        self
    }

    fn entry_content<'a>(
        &'a self,
        property: &PropertyRecord,
        matches: impl Fn(&EntryContents) -> bool,
    ) -> Option<&'a EntryContents> {
        let mut contents = self
            .entries
            .iter()
            .filter(|entry| {
                property
                    .side_entries()
                    .iter()
                    .any(|name| name == entry.name())
            })
            .flat_map(EntryRecord::contents)
            .filter(|content| matches(content));
        let content = contents.next()?;
        contents.next().is_none().then_some(content)
    }

    pub(crate) fn mesh_material_slots(
        &self,
        property: &PropertyRecord,
    ) -> Option<MeshMaterialSlots<'_>> {
        if property.type_name != "Mesh::PropertyMaterial" {
            return None;
        }
        let EntryContents::MeshMaterial {
            binding,
            ambient,
            diffuse,
            specular,
            emissive,
            shininess,
            transparency,
        } = self.entry_content(property, |content| {
            matches!(content, EntryContents::MeshMaterial { .. })
        })?
        else {
            return None;
        };
        Some(MeshMaterialSlots {
            binding: *binding,
            colors: [
                ambient.as_deref(),
                diffuse.as_deref(),
                specular.as_deref(),
                emissive.as_deref(),
            ],
            shininess: shininess.as_deref(),
            transparency: transparency.as_deref(),
        })
    }

    pub(crate) fn float_slots(&self, property: &PropertyRecord) -> Option<&[Option<FiniteReal>]> {
        if property.type_name != "App::PropertyFloatList" {
            return None;
        }
        match self.entry_content(property, |content| {
            matches!(content, EntryContents::Float { .. })
        })? {
            EntryContents::Float { values } => Some(values),
            _ => None,
        }
    }
    pub(crate) fn color_slots(&self, property: &PropertyRecord) -> Option<&[Option<u32>]> {
        if property.type_name != "App::PropertyColorList" {
            return None;
        }
        match self.entry_content(property, |content| {
            matches!(content, EntryContents::Color { .. })
        })? {
            EntryContents::Color { values } => Some(values),
            _ => None,
        }
    }
    pub(crate) fn vector_slots(
        &self,
        property: &PropertyRecord,
    ) -> Option<&[Option<[FiniteReal; 3]>]> {
        if property.type_name != "App::PropertyVectorList" {
            return None;
        }
        match self.entry_content(property, |content| {
            matches!(content, EntryContents::Vector { .. })
        })? {
            EntryContents::Vector { values, .. } => Some(values),
            _ => None,
        }
    }
    pub(crate) fn placement_slots(
        &self,
        property: &PropertyRecord,
    ) -> Option<&[Option<[FiniteReal; 7]>]> {
        if property.type_name != "App::PropertyPlacementList" {
            return None;
        }
        match self.entry_content(property, |content| {
            matches!(content, EntryContents::Placement { .. })
        })? {
            EntryContents::Placement { values, .. } => Some(values),
            _ => None,
        }
    }
    pub(crate) fn fillet_slots(&self, property: &PropertyRecord) -> Option<&[Option<FilletValue>]> {
        if property.type_name != "Part::PropertyFilletEdges" {
            return None;
        }
        match self.entry_content(property, |content| {
            matches!(content, EntryContents::Fillet { .. })
        })? {
            EntryContents::Fillet { values } => Some(values),
            _ => None,
        }
    }

    pub(crate) fn f32_slots(&self, property: &PropertyRecord) -> Option<&F32Values> {
        let components = match property.type_name.as_str() {
            "Points::PropertyGreyValueList" | "Inspection::PropertyDistanceList" => 1,
            "Points::PropertyNormalList" | "Mesh::PropertyNormalList" => 3,
            "Points::PropertyCurvatureList" | "Mesh::PropertyCurvatureList" => 8,
            _ => return None,
        };
        match self.entry_content(property, |content| match content {
            EntryContents::F32 {
                values: F32Values::Scalar(_),
            } => components == 1,
            EntryContents::F32 {
                values: F32Values::Vector(_),
            } => components == 3,
            EntryContents::F32 {
                values: F32Values::Curvature(_),
            } => components == 8,
            _ => false,
        })? {
            EntryContents::F32 { values } => Some(values),
            _ => None,
        }
    }

    pub(super) fn f32_present(
        &self,
        property: &PropertyRecord,
        ordinal: usize,
        components: usize,
    ) -> bool {
        self.f32_slots(property).is_some_and(|values| match values {
            F32Values::Scalar(values) => {
                components == 1 && values.get(ordinal).is_some_and(Option::is_some)
            }
            F32Values::Vector(values) => {
                components == 3 && values.get(ordinal).is_some_and(Option::is_some)
            }
            F32Values::Curvature(values) => {
                components == 8 && values.get(ordinal).is_some_and(Option::is_some)
            }
        })
    }

    pub(crate) fn payload_ordinals<'a>(
        &'a self,
        property: &'a PropertyRecord,
    ) -> impl Iterator<Item = usize> + 'a + use<'a, 'ctx, 'arena> {
        let count = self
            .float_slots(property)
            .map(<[_]>::len)
            .or_else(|| self.color_slots(property).map(<[_]>::len))
            .or_else(|| self.vector_slots(property).map(<[_]>::len))
            .or_else(|| self.placement_slots(property).map(<[_]>::len))
            .or_else(|| self.fillet_slots(property).map(<[_]>::len))
            .or_else(|| {
                self.f32_slots(property).map(|values| match values {
                    F32Values::Scalar(values) => values.len(),
                    F32Values::Vector(values) => values.len(),
                    F32Values::Curvature(values) => values.len(),
                })
            })
            .unwrap_or(0);
        (0..count).filter(move |ordinal| {
            self.f32_present(property, *ordinal, 1)
                || self.f32_present(property, *ordinal, 3)
                || self.f32_present(property, *ordinal, 8)
                || self
                    .float_slots(property)
                    .is_some_and(|values| values[*ordinal].is_some())
                || self
                    .color_slots(property)
                    .is_some_and(|values| values[*ordinal].is_some())
                || self
                    .vector_slots(property)
                    .is_some_and(|values| values[*ordinal].is_some())
                || self
                    .placement_slots(property)
                    .is_some_and(|values| values[*ordinal].is_some())
                || self
                    .fillet_slots(property)
                    .is_some_and(|values| values[*ordinal].is_some())
        })
    }
}
