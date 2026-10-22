// SPDX-License-Identifier: Apache-2.0
//! Complete source inventories for the adopted XML and opaque-member grammars.

use std::collections::BTreeMap;
use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::b1::{classification::ClassificationIndex, partition_stream};
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{CadIr, SourceFidelity};

const INVENTORY: &str = "FreeCAD source framing inventory";

pub(crate) enum Admission {
    Complete(Inventory),
    Pending,
}

impl Admission {
    pub(crate) fn is_complete(&self) -> bool {
        matches!(self, Self::Complete(_))
    }
}

#[derive(Default)]
struct Stream {
    byte_len: u64,
    framing: Vec<Range<u64>>,
    insignificant: Vec<Range<u64>>,
}

pub(crate) struct Inventory(BTreeMap<String, Stream>);

pub(crate) fn prepare(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    scan: &crate::container::Scan<'_>,
) -> Result<Admission, CodecError> {
    if scan.data.contains_key("GuiDocument.xml") || pending_grammar(&scan.document_xml) {
        return Ok(Admission::Pending);
    }
    let mut actual = ctx.source_streams()?;
    actual.sort_unstable_by(|left, right| left.name().cmp(right.name()));
    let mut inventory = Inventory(BTreeMap::new());
    inventory.stream(ctx, root, &actual)?;
    for source in scan.data.values() {
        inventory.stream(ctx, *source, &actual)?;
    }
    if inventory.0.len() != actual.len() {
        return Ok(Admission::Pending);
    }
    let frames = match scan.archive.framing_views(ctx) {
        Ok(frames) => frames,
        Err(CodecError::Malformed(_) | CodecError::Truncated { .. }) => {
            return Ok(Admission::Pending);
        }
        Err(error) => return Err(error),
    };
    for view in frames {
        inventory.range(ctx, view, false)?;
    }
    let document = *scan.data.get("Document.xml").ok_or_else(|| {
        CodecError::malformed("source framing requires its genuine Document.xml member")
    })?;
    let xml = crate::xml_framing::frame(ctx, document, &scan.document_xml, false)?;
    if !xml.supported() {
        return Ok(Admission::Pending);
    }
    for view in xml.framing() {
        inventory.range(ctx, *view, false)?;
    }
    for view in xml.insignificant() {
        inventory.range(ctx, *view, true)?;
    }
    Ok(Admission::Complete(inventory))
}

fn pending_grammar(xml: &roxmltree::Document<'_>) -> bool {
    xml.descendants()
        .filter(|node| node.is_element())
        .any(|node| {
            matches!(
                node.tag_name().name(),
                "StringHasher" | "StringHasher2" | "ElementMap" | "ElementMap2"
            ) || matches!(node.tag_name().name(), "Property" | "_Property")
                && node.attribute("type").is_some_and(|kind| {
                    matches!(
                        kind,
                        "Part::PropertyPartShape"
                            | "Mesh::PropertyMeshKernel"
                            | "Points::PropertyPointKernel"
                            | "App::PropertyColorList"
                            | "App::PropertyMaterialList"
                            | "App::PropertyFloatList"
                            | "App::PropertyVectorList"
                            | "App::PropertyPlacementList"
                            | "Part::PropertyFilletEdges"
                            | "Points::PropertyGreyValueList"
                            | "Inspection::PropertyDistanceList"
                            | "Points::PropertyNormalList"
                            | "Mesh::PropertyNormalList"
                            | "Points::PropertyCurvatureList"
                            | "Mesh::PropertyCurvatureList"
                            | "Mesh::PropertyMaterial"
                    )
                })
        })
}

impl Inventory {
    fn stream(
        &mut self,
        ctx: &DecodeContext<'_>,
        source: View<'_>,
        actual: &[cadmpeg_core::decode::SourceStream],
    ) -> Result<(), CodecError> {
        let capture = ctx.capture_part_extent(source)?;
        let at = actual
            .binary_search_by(|stream| stream.name().cmp(capture.stream()))
            .map_err(|_| CodecError::malformed("source framing has no actual registered stream"))?;
        let actual = &actual[at];
        if capture.offset() != 0 || capture.byte_len() != actual.byte_len() {
            return Err(CodecError::malformed(
                "source framing requires the whole actual stream View",
            ));
        }
        if let Some(previous) = self.0.get(capture.stream()) {
            if previous.byte_len != actual.byte_len() {
                return Err(CodecError::malformed(
                    "conflicting source framing stream length",
                ));
            }
            return Ok(());
        }
        let bytes = capture
            .stream()
            .len()
            .checked_add(size_of::<(String, Stream)>())
            .map(u64_from_index)
            .ok_or_else(|| CodecError::malformed("source framing stream storage overflows"))?;
        ctx.retain_bytes(bytes, INVENTORY)?;
        let mut name = String::new();
        name.try_reserve_exact(capture.stream().len())
            .map_err(|_| {
                ResourceLimit::allocation_failed(
                    ResourceDimension::RetainedBytes,
                    ctx.policy().limits.max_retained_bytes,
                    bytes,
                    INVENTORY,
                )
            })?;
        name.push_str(capture.stream());
        self.0.insert(
            name,
            Stream {
                byte_len: actual.byte_len(),
                ..Stream::default()
            },
        );
        Ok(())
    }

    fn range(
        &mut self,
        ctx: &DecodeContext<'_>,
        view: View<'_>,
        insignificant: bool,
    ) -> Result<(), CodecError> {
        let capture = ctx.capture_part_extent(view)?;
        let stream = self.0.get_mut(capture.stream()).ok_or_else(|| {
            CodecError::malformed("source framing range has no admitted actual stream")
        })?;
        let end = capture
            .offset()
            .checked_add(capture.byte_len())
            .filter(|end| *end <= stream.byte_len)
            .ok_or_else(|| {
                CodecError::malformed("source framing range escapes its actual stream")
            })?;
        if capture.byte_len() == 0 {
            return Ok(());
        }
        let ranges = if insignificant {
            &mut stream.insignificant
        } else {
            &mut stream.framing
        };
        reserve(ctx, ranges, 1)?;
        ranges.push(capture.offset()..end);
        Ok(())
    }

    fn is_complete(&self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        let actual = ctx.source_streams()?;
        Ok(actual.len() == self.0.len()
            && actual.iter().all(|stream| {
                self.0
                    .get(stream.name())
                    .is_some_and(|entry| entry.byte_len == stream.byte_len())
            }))
    }

    pub(crate) fn finalize(
        self,
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
        fidelity: &mut SourceFidelity,
        losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    ) -> Result<(), CodecError> {
        if !self.is_complete(ctx)? {
            return Err(CodecError::malformed(
                "B1 framing inventory no longer covers every actual stream",
            ));
        }
        let mut given_up = BTreeMap::<&str, Vec<Range<u64>>>::new();
        for part in fidelity
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::GivenUp {})
        {
            if !self.0.contains_key(part.stream()) {
                return Err(CodecError::malformed(
                    "given-up part has no actual framing stream",
                ));
            }
            let end = part
                .offset()
                .checked_add(part.byte_len())
                .ok_or_else(|| CodecError::malformed("given-up source extent overflows"))?;
            let ranges = match given_up.entry(part.stream()) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    ctx.retain_bytes(
                        u64_from_index(size_of::<(&str, Vec<Range<u64>>)>()),
                        INVENTORY,
                    )?;
                    entry.insert(Vec::new())
                }
            };
            reserve(ctx, ranges, 1)?;
            ranges.push(part.offset()..end);
        }
        let mut partitions = Vec::new();
        reserve(ctx, &mut partitions, self.0.len())?;
        for (name, stream) in &self.0 {
            let failed = given_up.get(name.as_str()).map_or(&[][..], Vec::as_slice);
            partitions.push(partition_stream(
                ctx,
                name,
                stream.byte_len,
                fidelity.record_layout(),
                &stream.framing,
                &stream.insignificant,
                failed,
            )?);
        }
        let owners = ClassificationIndex::new_with_owner_resolver(ctx, ir, &super::owners::Owners)?;
        fidelity.finalize_b1(ctx, &owners, "fcstd", &partitions, losses)
    }
}

fn reserve<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    additional: usize,
) -> Result<(), CodecError> {
    let bytes = u64_from_index(additional)
        .checked_mul(u64_from_index(size_of::<T>()))
        .ok_or_else(|| CodecError::malformed("source framing inventory allocation overflows"))?;
    ctx.retain_bytes(bytes, INVENTORY)?;
    values.try_reserve(additional).map_err(|_| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            bytes,
            INVENTORY,
        )
        .into()
    })
}

#[cfg(test)]
mod tests;
