// SPDX-License-Identifier: Apache-2.0
//! Resumable availability checks for an immutable, growing CFB prefix.

use std::collections::BTreeSet;
use std::ops::Bound::{Excluded, Unbounded};

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;

use super::{CompoundVersion, DIFAT_SECTOR, END_OF_CHAIN, FAT_SECTOR, FREE_SECTOR, MAGIC};

pub(super) enum PrefixDirectoryAvailability<'ctx> {
    NotCompound,
    Incomplete,
    Malformed(&'static str),
    Ready {
        version: CompoundVersion,
        available: usize,
        directory_chain: Vec<u32>,
        storage: ScopedReservation<'ctx>,
    },
}

pub(super) struct PrefixProbe<'ctx> {
    version: Option<CompoundVersion>,
    tables: Option<PrefixTables<'ctx>>,
}

struct PrefixTables<'ctx> {
    version: CompoundVersion,
    fat_count: usize,
    directory_sector_count: u32,
    difat_count: usize,
    next_difat: u32,
    walked_difat: usize,
    difat_visit: bool,
    difat_complete: bool,
    fat_sectors: Option<(Vec<u32>, ScopedReservation<'ctx>)>,
    seen_difat: Option<(BTreeSet<u32>, ScopedReservation<'ctx>)>,
    fat: (Vec<u32>, ScopedReservation<'ctx>),
    loaded_fat_count: usize,
    fat_visit: bool,
    checked_fat_roles: usize,
    fat_role_visit: bool,
    fat_role_end: bool,
    last_difat_role: Option<u32>,
    pending_difat_role: Option<u32>,
    difat_role_end: bool,
    directory_chain: (Vec<u32>, ScopedReservation<'ctx>),
    seen_directory: (BTreeSet<u32>, ScopedReservation<'ctx>),
    current_directory: u32,
    directory_visit: bool,
}

impl<'ctx> PrefixProbe<'ctx> {
    pub(super) fn new() -> Self {
        Self { version: None, tables: None }
    }

    /// Advances only over bytes and allocation words not checked before.
    /// Prefix extensions must preserve all previously supplied bytes.
    pub(super) fn advance(
        &mut self,
        ctx: &'ctx DecodeContext<'_>,
        prefix: View<'_>,
    ) -> Result<PrefixDirectoryAvailability<'ctx>, CodecError> {
        let prefix = prefix.window();
        if self.tables.is_none() {
    if prefix.get(..8) != Some(&MAGIC) {
        return Ok(PrefixDirectoryAvailability::NotCompound);
    }
    let version = match self.version {
        Some(version) => version,
        None => {
    let Some(major) = View::u16_le_at(prefix, 26) else {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    };
    let Some(shift) = View::u16_le_at(prefix, 30) else {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    };
    let Some(version) = CompoundVersion::from_header(major, shift) else {
        return Ok(PrefixDirectoryAvailability::Malformed(
            "invalid CFB sector layout",
        ));
    };
            self.version = Some(version);
            version
        }
    };
    let sector_size = version.sector_size();
    if prefix.len() < sector_size {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    }
    if prefix.get(8..24) != Some(&[0; 16])
        || View::u16_le_at(prefix, 24) != Some(0x003e)
        || View::u16_le_at(prefix, 28) != Some(0xfffe)
        || View::u16_le_at(prefix, 32) != Some(6)
        || prefix.get(34..40) != Some(&[0; 6])
        || View::u32_le_at(prefix, 56) != Some(4096)
    {
        return Ok(PrefixDirectoryAvailability::Malformed("invalid CFB header"));
    }
    if version == CompoundVersion::V4 && prefix[512..4096].iter().any(|byte| *byte != 0) {
        return Ok(PrefixDirectoryAvailability::Malformed(
            "CFB v4 header padding is not zero",
        ));
    }
    let Some(fat_count) = View::u32_le_at(prefix, 44).and_then(|v| usize::try_from(v).ok()) else {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    };
    let Some(directory_start) = View::u32_le_at(prefix, 48) else {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    };
    let Some(directory_sector_count) = View::u32_le_at(prefix, 40) else {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    };
    let Some(difat_start) = View::u32_le_at(prefix, 68) else {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    };
    let Some(difat_count) = View::u32_le_at(prefix, 72).and_then(|v| usize::try_from(v).ok())
    else {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    };
    if fat_count == 0
        || (version == CompoundVersion::V3 && directory_sector_count != 0)
        || (version == CompoundVersion::V4 && directory_sector_count == 0)
    {
        return Ok(PrefixDirectoryAvailability::Malformed(
            "invalid CFB header counts",
        ));
    }

    let mut fat_sectors = ctx.scoped_vector_storage(0, "probe CFB FAT sectors")?;
    let mut header_free_seen = false;
    for index in 0..109 {
        let Some(id) = View::u32_le_at(prefix, 76 + index * 4) else {
            return Ok(PrefixDirectoryAvailability::Incomplete);
        };
        if id == FREE_SECTOR {
            header_free_seen = true;
        } else {
            if header_free_seen {
                return Ok(PrefixDirectoryAvailability::Malformed(
                    "non-free CFB header DIFAT entry follows a free entry",
                ));
            }
            if fat_sectors.0.len() == fat_count {
                return Ok(PrefixDirectoryAvailability::Malformed(
                    "CFB FAT ids exceed the declared count",
                ));
            }
            ctx.push_scoped_vec(
                &mut fat_sectors.1,
                &mut fat_sectors.0,
                id,
                "probe CFB FAT sectors",
            )?;
        }
    }
        self.tables = Some(PrefixTables {
            version,
            fat_count,
            directory_sector_count,
            difat_count,
            next_difat: difat_start,
            walked_difat: 0,
            difat_visit: false,
            difat_complete: false,
            fat_sectors: Some(fat_sectors),
            seen_difat: Some((BTreeSet::new(), ctx.reserve_scoped(0, "CFB probe visits")?)),
            fat: (Vec::new(), ctx.reserve_scoped(0, "CFB probe FAT words")?),
            loaded_fat_count: 0,
            fat_visit: false,
            checked_fat_roles: 0,
            fat_role_visit: false,
            fat_role_end: false,
            last_difat_role: None,
            pending_difat_role: None,
            difat_role_end: false,
            directory_chain: (Vec::new(), ctx.reserve_scoped(0, "CFB probe directory chain")?),
            seen_directory: (BTreeSet::new(), ctx.reserve_scoped(0, "CFB probe visits")?),
            current_directory: directory_start,
            directory_visit: false,
        });
        }
        let tables = self.tables.as_mut().ok_or_else(|| CodecError::malformed("CFB probe state is absent"))?;
        tables.advance(ctx, prefix)
    }
}

impl<'ctx> PrefixTables<'ctx> {
    fn advance(
        &mut self,
        ctx: &'ctx DecodeContext<'_>,
        prefix: &[u8],
    ) -> Result<PrefixDirectoryAvailability<'ctx>, CodecError> {
        let sector_size = self.version.sector_size();
        let available = (prefix.len() - sector_size) / sector_size;
        if !self.difat_complete {
            let fat_sectors = self.fat_sectors.as_mut().ok_or_else(|| CodecError::malformed("CFB probe FAT ids are absent"))?;
            let seen_difat = self.seen_difat.as_mut().ok_or_else(|| CodecError::malformed("CFB probe DIFAT visits are absent"))?;
            let difat_entries = sector_size / 4 - 1;
            while self.walked_difat < self.difat_count {
                if !self.difat_visit {
                    ctx.charge_work(1, "visit CFB probe DIFAT sector")?;
                    self.difat_visit = true;
                }
                if cadmpeg_core::decode::index_from_u32(self.next_difat) >= available {
                    return Ok(PrefixDirectoryAvailability::Incomplete);
                }
                if !ctx.insert_scoped_btree_set(
                    &mut seen_difat.1, &mut seen_difat.0, self.next_difat,
                    "CFB probe DIFAT visits", "CFB probe DIFAT visits",
                )? {
                    return Ok(PrefixDirectoryAvailability::Malformed("CFB DIFAT chain is cyclic"));
                }
                let start = (cadmpeg_core::decode::index_from_u32(self.next_difat) + 1) * sector_size;
                let raw = &prefix[start..start + sector_size];
                let mut free_seen = false;
                for index in 0..difat_entries {
                    ctx.charge_work(1, "visit CFB DIFAT entries")?;
                    let Some(id) = View::u32_le_at(raw, index * 4) else {
                        return Ok(PrefixDirectoryAvailability::Incomplete);
                    };
                    if id == FREE_SECTOR {
                        free_seen = true;
                    } else {
                        if free_seen {
                            return Ok(PrefixDirectoryAvailability::Malformed("non-free CFB DIFAT entry follows a free entry"));
                        }
                        if fat_sectors.0.len() == self.fat_count {
                            return Ok(PrefixDirectoryAvailability::Malformed("CFB FAT ids exceed the declared count"));
                        }
                        ctx.push_scoped_vec(&mut fat_sectors.1, &mut fat_sectors.0, id, "probe CFB FAT sectors")?;
                    }
                }
                self.next_difat = View::u32_le_at(raw, difat_entries * 4)
                    .ok_or_else(|| CodecError::malformed("CFB probe DIFAT link is truncated"))?;
                self.walked_difat += 1;
                self.difat_visit = false;
            }
            if self.next_difat != END_OF_CHAIN {
                return Ok(PrefixDirectoryAvailability::Malformed("CFB DIFAT chain length does not match the header"));
            }
            if fat_sectors.0.len() != self.fat_count {
                return Ok(PrefixDirectoryAvailability::Malformed("CFB DIFAT does not match its declared FAT count"));
            }
            self.difat_complete = true;
        }

        if let Some(fat_sectors) = &mut self.fat_sectors {
            while self.loaded_fat_count < self.fat_count {
                if !self.fat_visit {
                    ctx.charge_work(1, "visit CFB FAT sectors")?;
                    self.fat_visit = true;
                }
                let id = fat_sectors.0[self.loaded_fat_count];
                if cadmpeg_core::decode::index_from_u32(id) >= available {
                    break;
                }
                let start = (cadmpeg_core::decode::index_from_u32(id) + 1) * sector_size;
                let raw = &prefix[start..start + sector_size];
                ctx.reserve_scoped_vec(&mut self.fat.1, &mut self.fat.0, raw.len() / 4, "CFB probe FAT words")?;
                for word in ctx.admit_iter(raw.as_chunks::<4>().0, "decode CFB probe FAT words")? {
                    self.fat.1.with_storage(|| ctx.reserve_capacity(&mut self.fat.0, 1, "CFB probe FAT word slot"))?;
                    self.fat.0.push(View::u32_le_at(word, 0).ok_or_else(|| CodecError::malformed("CFB probe FAT word is truncated"))?);
                }
                self.loaded_fat_count += 1;
                self.fat_visit = false;
            }
            while self.checked_fat_roles < self.loaded_fat_count {
                if !self.fat_role_visit {
                    ctx.charge_work(1, "check CFB probe FAT roles")?;
                    self.fat_role_visit = true;
                }
                let id = fat_sectors.0[self.checked_fat_roles];
                match self.fat.0.get(cadmpeg_core::decode::index_from_u32(id)) {
                    None => return Ok(PrefixDirectoryAvailability::Incomplete),
                    Some(&FAT_SECTOR) => {},
                    Some(_) => return Ok(PrefixDirectoryAvailability::Malformed("CFB allocation sector has the wrong role marker")),
                }
                self.checked_fat_roles += 1;
                self.fat_role_visit = false;
            }
            if !self.fat_role_end {
                ctx.charge_work(1, "check CFB probe FAT roles")?;
                self.fat_role_end = true;
            }
        }
        if let Some(seen_difat) = &self.seen_difat {
            loop {
                let id = match self.pending_difat_role {
                    Some(id) => id,
                    None => {
                        // Keep one end visit even when later FAT extensions add coverage.
                        if !self.difat_role_end {
                            ctx.charge_work(1, "check CFB probe DIFAT roles")?;
                        }
                        let lower = self.last_difat_role.map_or(Unbounded, Excluded);
                        let Some(&id) = seen_difat.0.range((lower, Unbounded)).next() else {
                            self.difat_role_end = true;
                            break;
                        };
                        self.pending_difat_role = Some(id);
                        id
                    }
                };
                match self.fat.0.get(cadmpeg_core::decode::index_from_u32(id)) {
                    None => return Ok(PrefixDirectoryAvailability::Incomplete),
                    Some(&DIFAT_SECTOR) => {},
                    Some(_) => return Ok(PrefixDirectoryAvailability::Malformed("CFB allocation sector has the wrong role marker")),
                }
                self.last_difat_role = Some(id);
                self.pending_difat_role = None;
            }
        }
        drop(self.seen_difat.take());
        if self.loaded_fat_count == self.fat_count {
            drop(self.fat_sectors.take());
        }

        loop {
            if !self.directory_visit {
                ctx.charge_work(1, "visit CFB probe directory chain")?;
                self.directory_visit = true;
            }
            let current = self.current_directory;
            if cadmpeg_core::decode::index_from_u32(current) >= available {
                return Ok(PrefixDirectoryAvailability::Incomplete);
            }
            let Some(&next) = self.fat.0.get(cadmpeg_core::decode::index_from_u32(current)) else {
                return Ok(if self.loaded_fat_count < self.fat_count {
                    PrefixDirectoryAvailability::Incomplete
                } else {
                    PrefixDirectoryAvailability::Malformed("CFB FAT does not address the directory sector")
                });
            };
            if !ctx.insert_scoped_btree_set(
                &mut self.seen_directory.1, &mut self.seen_directory.0, current,
                "CFB probe directory visits", "CFB probe directory visits",
            )? {
                return Ok(PrefixDirectoryAvailability::Malformed("CFB directory chain is cyclic"));
            }
            ctx.push_scoped_vec(&mut self.directory_chain.1, &mut self.directory_chain.0, current, "CFB probe directory chain")?;
            if next == END_OF_CHAIN {
                break;
            }
            if next >= DIFAT_SECTOR {
                return Ok(PrefixDirectoryAvailability::Malformed("CFB directory chain has an invalid terminator"));
            }
            self.current_directory = next;
            self.directory_visit = false;
        }
        if self.version == CompoundVersion::V4 && cadmpeg_core::decode::index_from_u32(self.directory_sector_count) != self.directory_chain.0.len() {
            return Ok(PrefixDirectoryAvailability::Malformed("CFB directory chain length does not match the header"));
        }
        let (directory_chain, storage) = std::mem::replace(
            &mut self.directory_chain,
            (Vec::new(), ctx.reserve_scoped(0, "CFB probe directory chain")?),
        );
        Ok(PrefixDirectoryAvailability::Ready { version: self.version, available, directory_chain, storage })
    }
}

pub(super) fn probe_directory_availability<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    prefix: View<'_>,
) -> Result<PrefixDirectoryAvailability<'ctx>, CodecError> {
    PrefixProbe::new().advance(ctx, prefix)
}

#[cfg(test)]
mod tests;
