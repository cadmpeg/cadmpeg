// SPDX-License-Identifier: Apache-2.0
//! Resumable availability checks for an immutable, growing CFB prefix.

use std::collections::BTreeSet;

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
    directory_sector_count: u32,
    difat: PrefixDifat<'ctx>,
    fat: PrefixFat<'ctx>,
    directory: PrefixDirectory<'ctx>,
}

struct PrefixDifat<'ctx> {
    count: usize,
    next: u32,
    walked: usize,
    visit: bool,
    sectors: Option<(BTreeSet<u32>, ScopedReservation<'ctx>)>,
    roles: Option<(
        std::collections::btree_set::IntoIter<u32>,
        ScopedReservation<'ctx>,
    )>,
    pending_role: Option<u32>,
}

struct PrefixFat<'ctx> {
    count: usize,
    sectors: Option<(Vec<u32>, ScopedReservation<'ctx>)>,
    words: (Vec<u32>, ScopedReservation<'ctx>),
    loaded_count: usize,
    visit: bool,
    checked_roles: usize,
    role_visit: bool,
    role_end: bool,
}

struct PrefixDirectory<'ctx> {
    chain: (Vec<u32>, ScopedReservation<'ctx>),
    seen: (BTreeSet<u32>, ScopedReservation<'ctx>),
    current: u32,
    visit: bool,
}

impl<'ctx> PrefixProbe<'ctx> {
    pub(super) fn new() -> Self {
        Self {
            version: None,
            tables: None,
        }
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
            let Some(fat_count) = View::u32_le_at(prefix, 44).and_then(|v| usize::try_from(v).ok())
            else {
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
            let Some(difat_count) =
                View::u32_le_at(prefix, 72).and_then(|v| usize::try_from(v).ok())
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
                directory_sector_count,
                difat: PrefixDifat {
                    count: difat_count,
                    next: difat_start,
                    walked: 0,
                    visit: false,
                    sectors: Some((BTreeSet::new(), ctx.reserve_scoped(0, "CFB probe visits")?)),
                    roles: None,
                    pending_role: None,
                },
                fat: PrefixFat {
                    count: fat_count,
                    sectors: Some(fat_sectors),
                    words: (Vec::new(), ctx.reserve_scoped(0, "CFB probe FAT words")?),
                    loaded_count: 0,
                    visit: false,
                    checked_roles: 0,
                    role_visit: false,
                    role_end: false,
                },
                directory: PrefixDirectory {
                    chain: (
                        Vec::new(),
                        ctx.reserve_scoped(0, "CFB probe directory chain")?,
                    ),
                    seen: (BTreeSet::new(), ctx.reserve_scoped(0, "CFB probe visits")?),
                    current: directory_start,
                    visit: false,
                },
            });
        }
        let tables = self
            .tables
            .as_mut()
            .ok_or_else(|| CodecError::malformed("CFB probe state is absent"))?;
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
        if self.difat.sectors.is_some() {
            let fat_sectors = self
                .fat
                .sectors
                .as_mut()
                .ok_or_else(|| CodecError::malformed("CFB probe FAT ids are absent"))?;
            let seen_difat = self
                .difat
                .sectors
                .as_mut()
                .ok_or_else(|| CodecError::malformed("CFB probe DIFAT visits are absent"))?;
            let difat_entries = sector_size / 4 - 1;
            while self.difat.walked < self.difat.count {
                if !self.difat.visit {
                    ctx.charge_work(1, "visit CFB probe DIFAT sector")?;
                    self.difat.visit = true;
                }
                if cadmpeg_core::decode::index_from_u32(self.difat.next) >= available {
                    return Ok(PrefixDirectoryAvailability::Incomplete);
                }
                if !ctx.insert_scoped_btree_set(
                    &mut seen_difat.1,
                    &mut seen_difat.0,
                    self.difat.next,
                    "CFB probe DIFAT visits",
                    "CFB probe DIFAT visits",
                )? {
                    return Ok(PrefixDirectoryAvailability::Malformed(
                        "CFB DIFAT chain is cyclic",
                    ));
                }
                let start =
                    (cadmpeg_core::decode::index_from_u32(self.difat.next) + 1) * sector_size;
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
                            return Ok(PrefixDirectoryAvailability::Malformed(
                                "non-free CFB DIFAT entry follows a free entry",
                            ));
                        }
                        if fat_sectors.0.len() == self.fat.count {
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
                self.difat.next = View::u32_le_at(raw, difat_entries * 4)
                    .ok_or_else(|| CodecError::malformed("CFB probe DIFAT link is truncated"))?;
                self.difat.walked += 1;
                self.difat.visit = false;
            }
            if self.difat.next != END_OF_CHAIN {
                return Ok(PrefixDirectoryAvailability::Malformed(
                    "CFB DIFAT chain length does not match the header",
                ));
            }
            if fat_sectors.0.len() != self.fat.count {
                return Ok(PrefixDirectoryAvailability::Malformed(
                    "CFB DIFAT does not match its declared FAT count",
                ));
            }
            let (sectors, storage) = self
                .difat
                .sectors
                .take()
                .ok_or_else(|| CodecError::malformed("CFB probe DIFAT visits are absent"))?;
            self.difat.roles = Some((sectors.into_iter(), storage));
        }

        if let Some(fat_sectors) = &mut self.fat.sectors {
            while self.fat.loaded_count < self.fat.count {
                if !self.fat.visit {
                    ctx.charge_work(1, "visit CFB FAT sectors")?;
                    self.fat.visit = true;
                }
                let id = fat_sectors.0[self.fat.loaded_count];
                if cadmpeg_core::decode::index_from_u32(id) >= available {
                    break;
                }
                let start = (cadmpeg_core::decode::index_from_u32(id) + 1) * sector_size;
                let raw = &prefix[start..start + sector_size];
                ctx.reserve_scoped_vec(
                    &mut self.fat.words.1,
                    &mut self.fat.words.0,
                    raw.len() / 4,
                    "CFB probe FAT words",
                )?;
                for word in ctx.admit_iter(raw.as_chunks::<4>().0, "decode CFB probe FAT words")? {
                    self.fat.words.1.with_storage(|| {
                        ctx.reserve_capacity(&mut self.fat.words.0, 1, "CFB probe FAT word slot")
                    })?;
                    self.fat.words.0.push(
                        View::u32_le_at(word, 0).ok_or_else(|| {
                            CodecError::malformed("CFB probe FAT word is truncated")
                        })?,
                    );
                }
                self.fat.loaded_count += 1;
                self.fat.visit = false;
            }
            while self.fat.checked_roles < self.fat.loaded_count {
                if !self.fat.role_visit {
                    ctx.charge_work(1, "check CFB probe FAT roles")?;
                    self.fat.role_visit = true;
                }
                let id = fat_sectors.0[self.fat.checked_roles];
                match self
                    .fat
                    .words
                    .0
                    .get(cadmpeg_core::decode::index_from_u32(id))
                {
                    None => return Ok(PrefixDirectoryAvailability::Incomplete),
                    Some(&FAT_SECTOR) => {}
                    Some(_) => {
                        return Ok(PrefixDirectoryAvailability::Malformed(
                            "CFB allocation sector has the wrong role marker",
                        ))
                    }
                }
                self.fat.checked_roles += 1;
                self.fat.role_visit = false;
            }
            if !self.fat.role_end {
                ctx.charge_work(1, "check CFB probe FAT roles")?;
                self.fat.role_end = true;
            }
        }
        if let Some((roles, _storage)) = &mut self.difat.roles {
            loop {
                let id = match self.difat.pending_role {
                    Some(id) => id,
                    None => {
                        let Some(id) = ctx.next_charged(roles, "check CFB probe DIFAT roles")?
                        else {
                            break;
                        };
                        self.difat.pending_role = Some(id);
                        id
                    }
                };
                match self
                    .fat
                    .words
                    .0
                    .get(cadmpeg_core::decode::index_from_u32(id))
                {
                    None => return Ok(PrefixDirectoryAvailability::Incomplete),
                    Some(&DIFAT_SECTOR) => {}
                    Some(_) => {
                        return Ok(PrefixDirectoryAvailability::Malformed(
                            "CFB allocation sector has the wrong role marker",
                        ))
                    }
                }
                self.difat.pending_role = None;
            }
        }
        drop(self.difat.roles.take());
        if self.fat.loaded_count == self.fat.count {
            drop(self.fat.sectors.take());
        }

        loop {
            if !self.directory.visit {
                ctx.charge_work(1, "visit CFB probe directory chain")?;
                self.directory.visit = true;
            }
            let current = self.directory.current;
            if cadmpeg_core::decode::index_from_u32(current) >= available {
                return Ok(PrefixDirectoryAvailability::Incomplete);
            }
            let Some(&next) = self
                .fat
                .words
                .0
                .get(cadmpeg_core::decode::index_from_u32(current))
            else {
                return Ok(if self.fat.loaded_count < self.fat.count {
                    PrefixDirectoryAvailability::Incomplete
                } else {
                    PrefixDirectoryAvailability::Malformed(
                        "CFB FAT does not address the directory sector",
                    )
                });
            };
            if !ctx.insert_scoped_btree_set(
                &mut self.directory.seen.1,
                &mut self.directory.seen.0,
                current,
                "CFB probe directory visits",
                "CFB probe directory visits",
            )? {
                return Ok(PrefixDirectoryAvailability::Malformed(
                    "CFB directory chain is cyclic",
                ));
            }
            ctx.push_scoped_vec(
                &mut self.directory.chain.1,
                &mut self.directory.chain.0,
                current,
                "CFB probe directory chain",
            )?;
            if next == END_OF_CHAIN {
                break;
            }
            if next >= DIFAT_SECTOR {
                return Ok(PrefixDirectoryAvailability::Malformed(
                    "CFB directory chain has an invalid terminator",
                ));
            }
            self.directory.current = next;
            self.directory.visit = false;
        }
        if self.version == CompoundVersion::V4
            && cadmpeg_core::decode::index_from_u32(self.directory_sector_count)
                != self.directory.chain.0.len()
        {
            return Ok(PrefixDirectoryAvailability::Malformed(
                "CFB directory chain length does not match the header",
            ));
        }
        let (directory_chain, storage) = std::mem::replace(
            &mut self.directory.chain,
            (
                Vec::new(),
                ctx.reserve_scoped(0, "CFB probe directory chain")?,
            ),
        );
        Ok(PrefixDirectoryAvailability::Ready {
            version: self.version,
            available,
            directory_chain,
            storage,
        })
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
