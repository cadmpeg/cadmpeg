// SPDX-License-Identifier: Apache-2.0
//! Original admitted list fields supply independent derived record producers.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, RecordOwner, SourceExtentProof, View};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;
pub(crate) mod appearance;
mod interpretations;
pub(crate) mod mesh_material;
pub(crate) use interpretations::EntryInterpretations;

fn working(ctx: &DecodeContext<'_>, bytes: u64, operation: &'static str) -> Result<(), CodecError> {
    let allowance = ctx.per_expand_allowance();
    if bytes > allowance {
        return Err(ctx.refuse_part_limit(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            allowance,
            0,
            bytes,
            operation,
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Kind {
    Placement,
    Vector,
    Float,
    Color,
    Fillet,
    F32(usize),
}

struct HeaderSource<'source> {
    entry: View<'source>,
    extent: View<'source>,
    proof: SourceExtentProof,
    kind: Kind,
}

struct RowSource<'source> {
    extent: View<'source>,
    proof: SourceExtentProof,
}

struct ArrayBinding<'source> {
    entry: View<'source>,
    header: View<'source>,
    kind: Kind,
    rows: Vec<(usize, View<'source>)>,
}

pub(crate) struct PayloadBinding<'source> {
    array: Option<ArrayBinding<'source>>,
    mesh: Option<mesh_material::Alias<'source>>,
}

/// Metadata returned by this producer's actual count getter.
pub(crate) struct ArrayHeader<'a> {
    property: &'a str,
    count: usize,
    scalar_bytes: usize,
    kind: Kind,
}
impl ArrayHeader<'_> {
    pub(crate) fn count(&self) -> usize {
        self.count
    }
}

#[derive(Default)]
pub(crate) struct PayloadSources<'source> {
    headers: BTreeMap<String, HeaderSource<'source>>,
    rows: BTreeMap<String, BTreeMap<usize, RowSource<'source>>>,
    mesh_materials: BTreeMap<String, mesh_material::Source<'source>>,
}

impl<'source> PayloadSources<'source> {
    pub(super) fn header_source(
        &mut self,
        ctx: &DecodeContext<'source>,
        property: &str,
        runtime: &str,
        entry: View<'source>,
        extent: View<'source>,
    ) -> Result<(), CodecError> {
        let kind = match runtime {
            "App::PropertyPlacementList" => Kind::Placement,
            "App::PropertyVectorList" => Kind::Vector,
            "App::PropertyFloatList" => Kind::Float,
            "App::PropertyColorList" => Kind::Color,
            "Part::PropertyFilletEdges" => Kind::Fillet,
            "Points::PropertyGreyValueList" | "Inspection::PropertyDistanceList" => Kind::F32(1),
            "Points::PropertyNormalList" | "Mesh::PropertyNormalList" => Kind::F32(3),
            "Points::PropertyCurvatureList" | "Mesh::PropertyCurvatureList" => Kind::F32(8),
            _ => return Ok(()),
        };
        self.insert_header(ctx, property, entry, extent, kind)
    }

    fn insert_header(
        &mut self,
        ctx: &DecodeContext<'source>,
        property: &str,
        entry: View<'source>,
        extent: View<'source>,
        kind: Kind,
    ) -> Result<(), CodecError> {
        working(
            ctx,
            u64_from_index(
                property
                    .len()
                    .checked_add(std::mem::size_of::<(String, HeaderSource<'_>)>())
                    .ok_or_else(|| {
                        CodecError::malformed("payload header supplier storage overflows")
                    })?,
            ),
            "FreeCAD original payload header supplier",
        )?;
        let proof = ctx.source_extent_proof(extent)?;
        if self
            .headers
            .insert(
                property.to_owned(),
                HeaderSource {
                    entry,
                    extent,
                    proof,
                    kind,
                },
            )
            .is_some()
        {
            return Err(CodecError::malformed(
                "payload repeats an original header supplier",
            ));
        }
        Ok(())
    }

    pub(super) fn row_source(
        &mut self,
        ctx: &DecodeContext<'source>,
        property: &str,
        runtime: &str,
        ordinal: usize,
        extent: View<'source>,
    ) -> Result<(), CodecError> {
        if !matches!(
            runtime,
            "App::PropertyPlacementList"
                | "App::PropertyVectorList"
                | "App::PropertyFloatList"
                | "App::PropertyColorList"
                | "Part::PropertyFilletEdges"
                | "Points::PropertyGreyValueList"
                | "Inspection::PropertyDistanceList"
                | "Points::PropertyNormalList"
                | "Mesh::PropertyNormalList"
                | "Points::PropertyCurvatureList"
                | "Mesh::PropertyCurvatureList"
        ) {
            return Ok(());
        }
        self.insert_row(ctx, property, ordinal, extent)
    }

    fn insert_row(
        &mut self,
        ctx: &DecodeContext<'source>,
        property: &str,
        ordinal: usize,
        extent: View<'source>,
    ) -> Result<(), CodecError> {
        if !self.rows.contains_key(property) {
            working(
                ctx,
                u64_from_index(
                    property
                        .len()
                        .checked_add(
                            std::mem::size_of::<(String, BTreeMap<usize, RowSource<'_>>)>(),
                        )
                        .ok_or_else(|| {
                            CodecError::malformed("payload binding storage overflows")
                        })?,
                ),
                "FreeCAD original payload row binding",
            )?;
            self.rows.insert(property.to_owned(), BTreeMap::new());
        }
        working(
            ctx,
            u64_from_index(std::mem::size_of::<(usize, RowSource<'_>)>()),
            "FreeCAD original payload row supplier",
        )?;
        let proof = ctx.source_extent_proof(extent)?;
        if self
            .rows
            .get_mut(property)
            .ok_or_else(|| CodecError::malformed("payload row binding is absent"))?
            .insert(ordinal, RowSource { extent, proof })
            .is_some()
        {
            return Err(CodecError::malformed(
                "payload repeats an original row supplier",
            ));
        }
        Ok(())
    }

    pub(crate) fn export_alias(
        &self,
        ctx: &DecodeContext<'source>,
        property: &str,
    ) -> Result<Option<PayloadBinding<'source>>, CodecError> {
        let mesh = self.export_mesh_alias(ctx, property)?;
        let Some(header) = self.headers.get(property) else {
            return Ok(mesh.map(|mesh| PayloadBinding {
                array: None,
                mesh: Some(mesh),
            }));
        };
        let rows = self.rows.get(property);
        let count = rows.map_or(0, BTreeMap::len);
        let bytes = u64_from_index(count)
            .checked_mul(u64_from_index(std::mem::size_of::<(usize, View<'_>)>()))
            .ok_or_else(|| CodecError::malformed("payload alias source storage overflows"))?;
        working(ctx, bytes, "FreeCAD source-only payload alias")?;
        let mut views = Vec::new();
        views.try_reserve_exact(count).map_err(|_| {
            cadmpeg_core::decode::ResourceLimit::allocation_failed(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                bytes,
                "FreeCAD source-only payload alias",
            )
        })?;
        if let Some(rows) = rows {
            for (ordinal, row) in rows {
                views.push((*ordinal, row.extent));
            }
        }
        Ok(Some(PayloadBinding {
            array: Some(ArrayBinding {
                entry: header.entry,
                header: header.extent,
                kind: header.kind,
                rows: views,
            }),
            mesh,
        }))
    }

    pub(crate) fn import_alias(
        &mut self,
        ctx: &DecodeContext<'source>,
        property: &str,
        binding: &PayloadBinding<'source>,
    ) -> Result<(), CodecError> {
        if let Some(binding) = &binding.array {
            self.insert_header(ctx, property, binding.entry, binding.header, binding.kind)?;
            for (ordinal, extent) in &binding.rows {
                self.insert_row(ctx, property, *ordinal, *extent)?;
            }
        }
        if let Some(mesh) = &binding.mesh {
            self.import_mesh_alias(ctx, property, mesh)?;
        }
        Ok(())
    }

    pub(crate) fn append(&mut self, mut other: Self) -> Result<(), CodecError> {
        if other.headers.keys().any(|id| self.headers.contains_key(id))
            || other.rows.keys().any(|id| self.rows.contains_key(id))
            || other
                .mesh_materials
                .keys()
                .any(|id| self.mesh_materials.contains_key(id))
        {
            return Err(CodecError::malformed(
                "payload suppliers repeat an admitted identity",
            ));
        }
        self.headers.append(&mut other.headers);
        self.rows.append(&mut other.rows);
        self.mesh_materials.append(&mut other.mesh_materials);
        Ok(())
    }

    fn read_header<'a>(
        &'a self,
        ctx: &DecodeContext<'source>,
        property: &'a str,
        source: &HeaderSource<'source>,
    ) -> Result<ArrayHeader<'a>, CodecError> {
        ctx.require_source_authority(&source.proof)?;
        ctx.require_source_ancestry(&source.proof, source.extent)?;
        let mut cursor = source.extent;
        let count = usize::try_from(cursor.req_u32_le()?).map_err(CodecError::malformed)?;
        if !cursor.is_empty() {
            return Err(CodecError::malformed(
                "array header supplier has an unexpected width",
            ));
        }
        let components = match source.kind {
            Kind::Placement => 7,
            Kind::Vector => 3,
            Kind::Float | Kind::Color | Kind::Fillet => 1,
            Kind::F32(components) => components,
        };
        let remaining = source
            .entry
            .window()
            .len()
            .checked_sub(source.extent.window().len())
            .ok_or_else(|| CodecError::malformed("array header exceeds its original entry"))?;
        let scalar_bytes = match source.kind {
            Kind::Float | Kind::Fillet => 8,
            Kind::Color | Kind::F32(_) => 4,
            Kind::Vector | Kind::Placement => {
                if count != 0 && count.checked_mul(components * 4) == Some(remaining) {
                    4
                } else {
                    8
                }
            }
        };
        Ok(ArrayHeader {
            property,
            count,
            scalar_bytes,
            kind: source.kind,
        })
    }

    pub(crate) fn header<'a>(
        &'a self,
        ctx: &DecodeContext<'source>,
        owner: &RecordOwner<'_>,
        property: &'a str,
    ) -> Result<Option<ArrayHeader<'a>>, CodecError> {
        let Some(source) = self.headers.get(property) else {
            return Ok(None);
        };
        ctx.record_fragment(owner, source.extent, || {
            self.read_header(ctx, property, source)
        })
        .map(Some)
    }

    pub(crate) fn begin_record<'a>(
        &'a self,
        ctx: &DecodeContext<'source>,
        id: &str,
        property: &'a str,
    ) -> Result<Option<(ArrayHeader<'a>, RecordOwner<'source>)>, CodecError> {
        let Some(source) = self.headers.get(property) else {
            return Ok(None);
        };
        ctx.record_owner(id, source.extent, || {
            self.read_header(ctx, property, source)
        })
        .map(Some)
    }

    fn read_row<const N: usize>(
        &self,
        ctx: &DecodeContext<'source>,
        owner: &RecordOwner<'_>,
        ordinal: usize,
        header: &ArrayHeader<'_>,
    ) -> Result<[f64; N], CodecError> {
        let source = self
            .rows
            .get(header.property)
            .and_then(|rows| rows.get(&ordinal))
            .ok_or_else(|| {
                CodecError::malformed("held array row has no admitted original supplier")
            })?;
        if ordinal >= header.count || source.extent.window().len() != N * header.scalar_bytes {
            return Err(CodecError::malformed(
                "array row differs from its actual count and width supplier",
            ));
        }
        ctx.require_source_authority(&source.proof)?;
        ctx.require_source_ancestry(&source.proof, source.extent)?;
        ctx.record_fragment(owner, source.extent, || {
            let mut cursor = source.extent;
            let mut values = [0.0; N];
            for value in &mut values {
                *value = super::read_real(&mut cursor, header.scalar_bytes == 4)?.get();
            }
            if !cursor.is_empty() {
                return Err(CodecError::malformed("array row getter left unread fields"));
            }
            Ok(values)
        })
    }

    pub(crate) fn fillet_radii(
        &self,
        ctx: &DecodeContext<'source>,
        owner: &RecordOwner<'_>,
        ordinal: usize,
        header: &ArrayHeader<'_>,
    ) -> Result<[f64; 2], CodecError> {
        if !matches!(header.kind, Kind::Fillet) {
            return Err(CodecError::malformed(
                "fillet getter has a non-fillet header",
            ));
        }
        let source = self
            .rows
            .get(header.property)
            .and_then(|rows| rows.get(&ordinal))
            .ok_or_else(|| CodecError::malformed("fillet radius supplier is absent"))?;
        if ordinal >= header.count || source.extent.window().len() != 20 {
            return Err(CodecError::malformed(
                "fillet radius supplier has a different grammar",
            ));
        }
        let extent = source
            .extent
            .child(source.extent.start() + 4, source.extent.end())
            .ok_or_else(|| CodecError::malformed("fillet radius extent escapes its row"))?;
        ctx.require_source_authority(&source.proof)?;
        ctx.require_source_ancestry(&source.proof, extent)?;
        ctx.record_fragment(owner, extent, || {
            let mut cursor = extent;
            Ok([
                super::read_real(&mut cursor, false)?.get(),
                super::read_real(&mut cursor, false)?.get(),
            ])
        })
    }

    pub(crate) fn placement_values(
        &self,
        ctx: &DecodeContext<'source>,
        owner: &RecordOwner<'_>,
        ordinal: usize,
        header: &ArrayHeader<'_>,
    ) -> Result<[cadmpeg_ir::scalar::FiniteReal; 7], CodecError> {
        if !matches!(header.kind, Kind::Placement) {
            return Err(CodecError::malformed(
                "placement getter has a non-placement header",
            ));
        }
        let raw = self.read_row::<7>(ctx, owner, ordinal, header)?;
        let mut values = [cadmpeg_ir::scalar::FiniteReal::ZERO; 7];
        for (value, raw) in values.iter_mut().zip(raw) {
            *value = cadmpeg_ir::scalar::FiniteReal::new(raw).ok_or_else(|| {
                CodecError::malformed("placement getter has a non-finite component")
            })?;
        }
        Ok(values)
    }

    pub(crate) fn placement(
        &self,
        ctx: &DecodeContext<'source>,
        owner: &RecordOwner<'_>,
        ordinal: usize,
        header: &ArrayHeader<'_>,
    ) -> Result<crate::native::frame::FiniteFrame, CodecError> {
        crate::placement::placement_components(
            &self
                .placement_values(ctx, owner, ordinal, header)?
                .map(cadmpeg_ir::scalar::FiniteReal::get),
        )
        .ok_or_else(|| {
            CodecError::malformed("admitted placement row no longer has a valid quaternion")
        })
    }

    pub(crate) fn float(
        &self,
        ctx: &DecodeContext<'source>,
        owner: &RecordOwner<'_>,
        ordinal: usize,
        header: &ArrayHeader<'_>,
    ) -> Result<cadmpeg_ir::scalar::FiniteReal, CodecError> {
        if !matches!(header.kind, Kind::Float | Kind::F32(1)) {
            return Err(CodecError::malformed("float getter has a non-float header"));
        }
        cadmpeg_ir::scalar::FiniteReal::new(self.read_row::<1>(ctx, owner, ordinal, header)?[0])
            .ok_or_else(|| CodecError::malformed("float getter has a non-finite value"))
    }

    pub(crate) fn color(
        &self,
        ctx: &DecodeContext<'source>,
        owner: &RecordOwner<'_>,
        ordinal: usize,
        header: &ArrayHeader<'_>,
    ) -> Result<u32, CodecError> {
        if !matches!(header.kind, Kind::Color) {
            return Err(CodecError::malformed("color getter has a non-color header"));
        }
        let source = self
            .rows
            .get(header.property)
            .and_then(|rows| rows.get(&ordinal))
            .ok_or_else(|| {
                CodecError::malformed("held color row has no admitted original supplier")
            })?;
        if ordinal >= header.count || source.extent.window().len() != 4 {
            return Err(CodecError::malformed(
                "color row differs from its actual header",
            ));
        }
        ctx.require_source_authority(&source.proof)?;
        ctx.require_source_ancestry(&source.proof, source.extent)?;
        ctx.record_fragment(owner, source.extent, || {
            let mut cursor = source.extent;
            Ok(cursor.req_u32_le()?)
        })
    }

    pub(crate) fn vector(
        &self,
        ctx: &DecodeContext<'source>,
        owner: &RecordOwner<'_>,
        ordinal: usize,
        header: &ArrayHeader<'_>,
    ) -> Result<cadmpeg_ir::units::FiniteVector<3>, CodecError> {
        if !matches!(header.kind, Kind::Vector | Kind::F32(3)) {
            return Err(CodecError::malformed(
                "vector getter has a placement header",
            ));
        }
        cadmpeg_ir::units::FiniteVector::new(self.read_row::<3>(ctx, owner, ordinal, header)?)
            .ok_or_else(|| CodecError::malformed("admitted vector row is not finite"))
    }
    pub(crate) fn curvature(
        &self,
        ctx: &DecodeContext<'source>,
        owner: &RecordOwner<'_>,
        ordinal: usize,
        header: &ArrayHeader<'_>,
    ) -> Result<cadmpeg_ir::units::FiniteVector<8>, CodecError> {
        if !matches!(header.kind, Kind::F32(8)) {
            return Err(CodecError::malformed(
                "curvature getter has a different grammar",
            ));
        }
        cadmpeg_ir::units::FiniteVector::new(self.read_row::<8>(ctx, owner, ordinal, header)?)
            .ok_or_else(|| CodecError::malformed("admitted curvature row is not finite"))
    }
}

#[derive(Default)]
pub(crate) struct DecodedPayloads<'source> {
    pub(crate) entry_contents: Vec<crate::native::entry_contents::EntryContentUpdate>,
    pub(crate) sources: PayloadSources<'source>,
}
