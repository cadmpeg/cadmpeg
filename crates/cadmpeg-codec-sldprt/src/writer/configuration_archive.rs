// SPDX-License-Identifier: Apache-2.0
//! The `Contents/Config-N` class map a written feature-input lane refers to.

use std::collections::BTreeMap;

use cadmpeg_core::decode::View;

use crate::records::{ArchiveParameterClass, FeatureInputLane};

/// The archive grammar version the authored stream states at offset 22.
const AUTHORED_ARCHIVE_VERSION: u32 = 17_000;
/// Zero bytes the stream opens with before its version.
const VERSION_OFFSET: usize = 22;
const CLASS_TAG_BIT: u16 = 0x8000;
const NAME_STRING: [u8; 4] = [0xff, 0xfe, 0xff, 0x00];

/// The minimal `Config-N` stream whose class map gives each archive-typed
/// scalar of `lane` its parameter class (`sldprt.md` §2.2).
///
/// The stream declares `moPart_c` and `moNodeName_c`, writes unnamed name
/// objects until the map reaches each scalar class's index, and declares that
/// class there. `None` when the lane states no map count, carries no
/// archive-typed scalar, or its indices admit no such stream.
pub(super) fn authored(lane: &FeatureInputLane) -> Option<Vec<u8>> {
    let payload = lane.native_payload.to_vec();
    if payload.get(2..4) == Some(&[0xff, 0xff][..]) {
        return None;
    }
    let count = View::u32_le_at(&payload, 0)?;
    let class_tag_at = |offset: u64| {
        let at = usize::try_from(offset).ok()?.checked_sub(2)?;
        let tag = View::u16_le_at(&payload, at)?;
        (tag & CLASS_TAG_BIT != 0 && tag != u16::MAX).then_some(tag & !CLASS_TAG_BIT)
    };
    let mut classes = BTreeMap::new();
    for scalar in &lane.scalars {
        let Some(class) = scalar.archive_class else {
            continue;
        };
        let name = lane.names.iter().find(|name| name.id == scalar.name)?;
        let index = class_tag_at(name.offset)?;
        if classes
            .insert(index, class_name(class))
            .is_some_and(|known| known != class_name(class))
        {
            return None;
        }
    }
    if classes.is_empty() {
        return None;
    }
    let name_class = View::u16_le_at(&payload, usize::try_from(lane.names.first()?.offset).ok()?)?;
    let name_class = name_class.checked_sub(CLASS_TAG_BIT)?;
    name_class.checked_sub(2).filter(|first| *first > 0)?;
    let mut stream = [0; VERSION_OFFSET].to_vec();
    stream.extend_from_slice(&AUTHORED_ARCHIVE_VERSION.to_le_bytes());
    declare(&mut stream, "moPart_c")?;
    stream.extend_from_slice(&[0, 0]);
    declare(&mut stream, "moNodeName_c")?;
    stream.extend_from_slice(&NAME_STRING);
    let mut next = u32::from(name_class) + 2;
    for (index, class) in classes {
        let index = u32::from(index);
        if index < next {
            return None;
        }
        while next < index {
            stream.extend_from_slice(&(CLASS_TAG_BIT | name_class).to_le_bytes());
            stream.extend_from_slice(&NAME_STRING);
            next += 1;
        }
        declare(&mut stream, class)?;
        next = index.checked_add(2)?;
    }
    (next == count).then_some(stream)
}

fn class_name(class: ArchiveParameterClass) -> &'static str {
    match class {
        ArchiveParameterClass::Length => "moLengthParameter_c",
        ArchiveParameterClass::Angle => "moAngleParameter_c",
        ArchiveParameterClass::Scaler => "moScalerParameter_c",
        ArchiveParameterClass::Integer => "moIntegerParameter_c",
    }
}

fn declare(stream: &mut Vec<u8>, name: &str) -> Option<()> {
    stream.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    stream.extend_from_slice(&u16::try_from(name.len()).ok()?.to_le_bytes());
    stream.extend_from_slice(name.as_bytes());
    Some(())
}
