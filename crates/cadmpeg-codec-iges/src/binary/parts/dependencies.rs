// SPDX-License-Identifier: Apache-2.0
//! Required Binary source-reference closure before semantic identity creation.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

use super::super::{
    counted_record, malformed, parameter_prefix, parse_directory_record, pointer_value,
    positive_pointer, source_sequence, BinaryDirectory, BinaryParameter, BinarySections,
};
use super::append;
use crate::directory::DirectoryFieldSlot;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Address {
    Directory(u32),
    Parameter(u32),
}

pub(super) struct Dependencies {
    live: BTreeSet<Address>,
    failed: BTreeSet<Address>,
    directory_sources: Vec<u32>,
    source_lines: Vec<u32>,
    source_points: Vec<u32>,
}

impl Dependencies {
    pub(super) fn inspect(
        sections: &BinarySections<'_>,
        global: &crate::global::ResolvedGlobal,
        ctx: &DecodeContext<'_>,
    ) -> Result<Self, CodecError> {
        let transaction = ctx.read_transaction()?;
        let mut graph = BTreeMap::new();
        let mut bad = BTreeSet::new();
        let mut directory_types = BTreeMap::new();
        let mut directory_sources = Vec::new();
        let mut discriminator_targets = Vec::new();
        let mut source_lines = Vec::new();
        let mut source_points = Vec::new();
        let source_integers = global.numeric_limits().integer_bits.is_none_or(|bits| {
            bits >= 8
                && bits
                    >= u32::from(
                        sections
                            .lengths
                            .single_integer
                            .max(sections.lengths.double_integer),
                    )
        });
        let mut cursor = 0;
        let mut ordinal = 0;
        while cursor < sections.directory.window().len() {
            let Some((offset, extent, body)) = bounded(sections.directory, cursor, "Directory")?
            else {
                for address in graph.keys().copied() {
                    insert(ctx, &mut bad, address)?;
                }
                break;
            };
            directory_sources.try_reserve(1).map_err(|_| {
                cadmpeg_core::decode::ResourceLimit::allocation_failed(
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                    ctx.policy().limits.max_retained_bytes,
                    u64_from_index(size_of::<u32>()),
                    "IGES Binary source sequence index",
                )
            })?;
            directory_sources.push(offset);
            let address = Address::Directory(offset);
            let sequence = source_sequence(ordinal)?;
            ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| malformed("Binary dependency ordinal overflows"))?;
            let mut targets = Vec::new();
            ctx.retain_bytes(
                u64_from_index(body.window().len()),
                "IGES Binary speculative Directory syntax",
            )?;
            match parse_directory_record(offset, sequence, body, sections.lengths) {
                Ok(record) => {
                    ctx.retain_bytes(
                        u64_from_index(size_of::<(u32, (i64, i64, bool, u32))>()),
                        "IGES Binary dependency type",
                    )?;
                    let point = record.has_point_parameters();
                    directory_types.insert(
                        offset,
                        (
                            record.entity_type(),
                            record.parameter_pointer(),
                            point,
                            sequence,
                        ),
                    );
                    if let super::super::BinaryValue::Pointer(pointer) =
                        &record.values[DirectoryFieldSlot::EntityType.slot()]
                    {
                        if *pointer != 0 {
                            if let Ok(target) = u32::try_from(pointer.unsigned_abs()) {
                                discriminator_targets.try_reserve(1).map_err(|_| {
                                    cadmpeg_core::decode::ResourceLimit::allocation_failed(
                                        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                                        ctx.policy().limits.max_retained_bytes,
                                        u64_from_index(size_of::<(Address, i64, u32, bool)>()),
                                        "IGES Binary discriminator source index",
                                    )
                                })?;
                                discriminator_targets.push((
                                    address,
                                    record.entity_type(),
                                    target,
                                    *pointer < 0,
                                ));
                            }
                        }
                    }
                    if let Err(error) = record
                        .admit_directory_fields()
                        .and_then(|()| collect_directory(&record, ctx, &mut targets))
                    {
                        propagate_resource(error)?;
                        insert(ctx, &mut bad, address)?;
                    }
                }
                Err(error) => {
                    propagate_resource(error)?;
                    insert(ctx, &mut bad, address)?;
                }
            }
            stage(ctx, &mut graph, address, targets)?;
            cursor = extent
                .end()
                .checked_sub(sections.directory.start())
                .ok_or_else(|| malformed("Binary dependency cursor underflows"))?;
        }
        cursor = 0;
        while cursor < sections.parameter.window().len() {
            let Some((offset, extent, body)) = bounded(sections.parameter, cursor, "Parameter")?
            else {
                for address in graph
                    .keys()
                    .copied()
                    .filter(|address| matches!(address, Address::Parameter(_)))
                {
                    insert(ctx, &mut bad, address)?;
                }
                break;
            };
            let address = Address::Parameter(offset);
            let mut targets = Vec::new();
            ctx.retain_bytes(
                u64_from_index(body.window().len()),
                "IGES Binary speculative Parameter syntax",
            )?;
            match parameter_prefix(body, sections.lengths) {
                Ok(mut prefix) => {
                    let mut source_point = false;
                    if let Ok(owner) =
                        positive_pointer(prefix.directory_pointer, "Parameter Directory")
                    {
                        append(ctx, &mut targets, Address::Directory(owner))?;
                        let declaration = directory_types.get(&owner);
                        if !declaration.is_some_and(|(entity_type, pointer, _, _)| {
                            *entity_type == prefix.entity_type && *pointer == i64::from(offset)
                        }) {
                            insert(ctx, &mut bad, address)?;
                        }
                        if declaration.is_some_and(|(_, _, point, _)| *point) {
                            source_point = prefix.entity_type == 116 && source_integers;
                            let mut coordinates = 0;
                            let mut symbol = None;
                            for field in 0..4 {
                                match prefix.stream.next() {
                                    Ok(Some(value)) => {
                                        if field < 3 {
                                            coordinates += 1;
                                            source_point &= matches!(
                                                value,
                                                super::super::BinaryValue::Integer(_)
                                                    | super::super::BinaryValue::Default
                                            );
                                        }
                                        if field == 3 {
                                            symbol = Some(value);
                                        }
                                    }
                                    Ok(None) => break,
                                    Err(error) => {
                                        propagate_resource(error)?;
                                        insert(ctx, &mut bad, address)?;
                                        break;
                                    }
                                }
                            }
                            source_point &= coordinates == 3 && symbol.is_some();
                            if let Some(value) = symbol {
                                source_point &=
                                    matches!(pointer_value(&value, "Point display symbol"), Ok(0));
                                match pointer_value(&value, "Point display symbol") {
                                    Ok(pointer) if pointer != 0 => {
                                        match directory_address(pointer) {
                                            Ok(target) => append(ctx, &mut targets, target)?,
                                            Err(error) => {
                                                propagate_resource(error)?;
                                                insert(ctx, &mut bad, address)?;
                                            }
                                        }
                                    }
                                    Ok(_) => {}
                                    Err(error) => {
                                        propagate_resource(error)?;
                                        insert(ctx, &mut bad, address)?;
                                    }
                                }
                            }
                        }
                    } else {
                        insert(ctx, &mut bad, address)?;
                    }
                    if !bad.contains(&address) {
                        let syntax = (|| {
                            let mut macro_values = Vec::new();
                            let mut source_line = prefix.entity_type == 110 && source_integers;
                            while let Some(value) = prefix.stream.next()? {
                                source_point = false;
                                source_line &= matches!(
                                    value,
                                    super::super::BinaryValue::Default
                                        | super::super::BinaryValue::Integer(_)
                                );
                                if prefix.entity_type == 306 {
                                    macro_values.try_reserve(1).map_err(|_| {
                                        cadmpeg_core::decode::ResourceLimit::allocation_failed(
                                            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                                            ctx.policy().limits.max_retained_bytes,
                                            u64_from_index(size_of::<super::super::BinaryValue>()),
                                            "IGES Binary Macro dependency syntax",
                                        )
                                    })?;
                                    macro_values.push(value);
                                }
                            }
                            prefix.stream.finish()?;
                            if source_line {
                                source_lines.try_reserve(1).map_err(|_| {
                                    cadmpeg_core::decode::ResourceLimit::allocation_failed(
                                        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                                        ctx.policy().limits.max_retained_bytes,
                                        u64_from_index(size_of::<u32>()),
                                        "IGES Binary Line source dispatch",
                                    )
                                })?;
                                source_lines.push(offset);
                            }
                            if source_point {
                                source_points.try_reserve(1).map_err(|_| {
                                    cadmpeg_core::decode::ResourceLimit::allocation_failed(
                                        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                                        ctx.policy().limits.max_retained_bytes,
                                        u64_from_index(size_of::<u32>()),
                                        "IGES Binary Point source dispatch",
                                    )
                                })?;
                                source_points.push(offset);
                            }
                            if prefix.entity_type == 306 {
                                let text = super::super::parameter_text(306, &macro_values)?;
                                crate::parameter::macro_parameter_data(
                                    &text,
                                    global.parameter_delimiter,
                                    global.record_delimiter,
                                )
                                .map_err(|error| match error {
                                    crate::parameter::MacroDataError::Error(error) => error,
                                    crate::parameter::MacroDataError::Defect(defect, _) => {
                                        malformed(defect.describe())
                                    }
                                })?;
                            }
                            Ok(())
                        })();
                        if let Err(error) = syntax {
                            propagate_resource(error)?;
                            insert(ctx, &mut bad, address)?;
                        }
                    }
                }
                Err(error) => {
                    propagate_resource(error)?;
                    insert(ctx, &mut bad, address)?;
                }
            }
            stage(ctx, &mut graph, address, targets)?;
            cursor = extent
                .end()
                .checked_sub(sections.parameter.start())
                .ok_or_else(|| malformed("Binary dependency cursor underflows"))?;
        }
        for (address, entity_type, target, negative) in discriminator_targets {
            let rendered = directory_types.get(&target).map(|(_, _, _, sequence)| {
                let value = i64::from(*sequence);
                if negative {
                    -value
                } else {
                    value
                }
            });
            if rendered != Some(entity_type) {
                insert(ctx, &mut bad, address)?;
            }
        }
        let mut reverse: BTreeMap<Address, Vec<Address>> = BTreeMap::new();
        for (owner, targets) in &graph {
            for target in targets {
                if !graph.contains_key(target) {
                    insert(ctx, &mut bad, *owner)?;
                }
                if !reverse.contains_key(target) {
                    ctx.retain_bytes(
                        u64_from_index(size_of::<(Address, Vec<Address>)>()),
                        "IGES Binary reverse dependency",
                    )?;
                    reverse.insert(*target, Vec::new());
                }
                let users = reverse
                    .get_mut(target)
                    .ok_or_else(|| malformed("Binary reverse dependency was not staged"))?;
                append(ctx, users, *owner)?;
            }
        }
        let mut queue = Vec::new();
        for address in &bad {
            append(ctx, &mut queue, *address)?;
        }
        let mut next = 0;
        while let Some(address) = queue.get(next).copied() {
            next = next
                .checked_add(1)
                .ok_or_else(|| malformed("Binary dependency queue overflows"))?;
            if let Some(users) = reverse.get(&address) {
                for user in users {
                    if !bad.contains(user) {
                        insert(ctx, &mut bad, *user)?;
                        append(ctx, &mut queue, *user)?;
                    }
                }
            }
        }
        let mut live = BTreeSet::new();
        for address in graph.into_keys().filter(|address| !bad.contains(address)) {
            insert(ctx, &mut live, address)?;
        }
        drop(transaction);
        Ok(Self {
            live,
            failed: bad,
            directory_sources,
            source_lines,
            source_points,
        })
    }

    pub(super) fn source_line(&self, offset: u32) -> bool {
        self.source_lines.binary_search(&offset).is_ok()
    }

    pub(super) fn source_point(&self, offset: u32) -> bool {
        self.source_points.binary_search(&offset).is_ok()
    }

    /// A Directory record needs only its transformation. Its Parameter Data
    /// record needs the Directory record, not the reverse: a given-up
    /// Parameter record leaves the Directory record with no parameters
    /// (#215 rule 1).
    pub(super) fn directory(&self, record: &BinaryDirectory) -> Result<(), CodecError> {
        self.require(Address::Directory(record.offset))?;
        let transform = pointer_value(
            &record.values[DirectoryFieldSlot::Transform.slot()],
            "Directory transformation",
        )?;
        if transform != 0 {
            self.require(directory_address(transform)?)?;
        }
        Ok(())
    }

    pub(super) fn parameter(
        &self,
        record: &BinaryParameter,
        owner: &BinaryDirectory,
    ) -> Result<(), CodecError> {
        self.require(Address::Parameter(record.offset))?;
        self.require(directory_address(record.directory_pointer)?)?;
        if owner.has_point_parameters() {
            if let Some(value) = record.values.get(3) {
                let pointer = pointer_value(value, "Point display symbol")?;
                if pointer != 0 {
                    self.require(directory_address(pointer)?)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn member_resolves_or_is_dropped(&self, pointer: i64) -> bool {
        if pointer == 0 {
            return true;
        }
        let Ok(offset) = u32::try_from(pointer.unsigned_abs()) else {
            return false;
        };
        self.live.contains(&Address::Directory(offset))
            || (pointer > 0 && self.failed.contains(&Address::Directory(offset)))
    }

    /// The source sequence of the given-up Directory record that a pointer
    /// value names by its source offset.
    pub(super) fn failed_member_sequence(&self, value: &super::super::BinaryValue) -> Option<u32> {
        let super::super::BinaryValue::Pointer(pointer) = value else {
            return None;
        };
        let offset = u32::try_from(*pointer)
            .ok()
            .filter(|pointer| *pointer != 0)?;
        if !self.failed.contains(&Address::Directory(offset)) {
            return None;
        }
        // Directory records are read in source order, so their offsets ascend.
        let ordinal = self.directory_sources.binary_search(&offset).ok()?;
        source_sequence(ordinal).ok()
    }

    fn require(&self, target: Address) -> Result<(), CodecError> {
        if self.live.contains(&target) {
            Ok(())
        } else {
            Err(malformed(
                "Binary required reference has no readable source target",
            ))
        }
    }
}

fn collect_directory(
    record: &BinaryDirectory,
    ctx: &DecodeContext<'_>,
    targets: &mut Vec<Address>,
) -> Result<(), CodecError> {
    let transform = pointer_value(
        &record.values[DirectoryFieldSlot::Transform.slot()],
        "Directory transformation",
    )?;
    if transform != 0 {
        append(ctx, targets, directory_address(transform)?)?;
    }
    Ok(())
}

fn directory_address(pointer: i64) -> Result<Address, CodecError> {
    u32::try_from(pointer.unsigned_abs())
        .map(Address::Directory)
        .map_err(|_| malformed("Binary source reference exceeds u32"))
}

fn bounded<'a>(
    source: View<'a>,
    cursor: usize,
    section: &str,
) -> Result<Option<(u32, View<'a>, View<'a>)>, CodecError> {
    match counted_record(source, cursor, section) {
        Ok(record) => Ok(Some(record)),
        Err(error) => {
            propagate_resource(error)?;
            Ok(None)
        }
    }
}

fn stage(
    ctx: &DecodeContext<'_>,
    graph: &mut BTreeMap<Address, Vec<Address>>,
    owner: Address,
    targets: Vec<Address>,
) -> Result<(), CodecError> {
    ctx.retain_bytes(
        u64_from_index(size_of::<(Address, Vec<Address>)>()),
        "IGES Binary source dependency",
    )?;
    graph.insert(owner, targets);
    Ok(())
}

fn insert(
    ctx: &DecodeContext<'_>,
    set: &mut BTreeSet<Address>,
    address: Address,
) -> Result<(), CodecError> {
    if !set.contains(&address) {
        ctx.retain_bytes(
            u64_from_index(size_of::<Address>()),
            "IGES Binary source liveness",
        )?;
        set.insert(address);
    }
    Ok(())
}

fn propagate_resource(error: CodecError) -> Result<(), CodecError> {
    match error {
        CodecError::ResourceLimit(_) => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
