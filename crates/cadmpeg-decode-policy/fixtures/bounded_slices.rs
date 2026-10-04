// SPDX-License-Identifier: Apache-2.0
use decode::budget::OptionalBudgetMethods;

pub struct DecodeContext;

pub mod decode {
    pub mod budget {
        pub struct DecodeBudget;
        pub struct WorkBudget {
            pub session: DecodeBudget,
        }

        impl WorkBudget {
            pub const INDEPENDENT_RECURSION_DEPTH: usize = 256;
        }

        pub struct ScopedReservation;

        impl DecodeBudget {
            pub fn charge_collection_items_limit(
                &self,
                _count: u64,
                _operation: &'static str,
            ) -> Result<(), ()> {
                Ok(())
            }

            pub fn charge_work_limit(
                &self,
                _count: u64,
                _operation: &'static str,
            ) -> Result<(), ()> {
                Ok(())
            }

            pub fn reserve_scoped_limit(
                &self,
                _bytes: u64,
                _operation: &'static str,
            ) -> Result<ScopedReservation, ()> {
                Ok(ScopedReservation)
            }
        }

        pub trait OptionalBudgetMethods {
            fn charge_work_limit(&self, count: u64, operation: &'static str) -> Result<(), ()>;
            fn reserve_scoped_limit(
                &self,
                bytes: u64,
                operation: &'static str,
            ) -> Result<ScopedReservation, ()>;
        }

        impl OptionalBudgetMethods for Option<&DecodeBudget> {
            fn charge_work_limit(&self, _count: u64, _operation: &'static str) -> Result<(), ()> {
                Ok(())
            }

            fn reserve_scoped_limit(
                &self,
                _bytes: u64,
                _operation: &'static str,
            ) -> Result<ScopedReservation, ()> {
                Ok(ScopedReservation)
            }
        }
    }

    pub mod work_scratch {
        pub struct WorkScratch {
            pub reservation: super::budget::ScopedReservation,
        }

        impl WorkScratch {
            pub fn from_reservation(reservation: super::budget::ScopedReservation) -> Self {
                Self { reservation }
            }
        }
    }
}

pub mod foreign {
    pub struct DecodeBudget;

    pub struct ScopedReservation;

    impl DecodeBudget {
        pub fn charge_work_limit(&self, _count: u64, _operation: &'static str) -> Result<(), ()> {
            Ok(())
        }

        pub fn reserve_scoped_limit(
            &self,
            _bytes: u64,
            _operation: &'static str,
        ) -> Result<ScopedReservation, ()> {
            Ok(ScopedReservation)
        }
    }
}

pub trait FakeBudgetMethods {
    fn charge_work_limit(&self, count: u64, operation: &'static str) -> Result<(), ()>;
    fn reserve_scoped_limit(&self, bytes: u64, operation: &'static str) -> Result<(), ()>;
}

impl FakeBudgetMethods for decode::budget::WorkBudget {
    fn charge_work_limit(&self, _count: u64, _operation: &'static str) -> Result<(), ()> {
        Ok(())
    }

    fn reserve_scoped_limit(&self, _bytes: u64, _operation: &'static str) -> Result<(), ()> {
        Ok(())
    }
}

impl FakeBudgetMethods for foreign::DecodeBudget {
    fn charge_work_limit(&self, _count: u64, _operation: &'static str) -> Result<(), ()> {
        Ok(())
    }

    fn reserve_scoped_limit(&self, _bytes: u64, _operation: &'static str) -> Result<(), ()> {
        Ok(())
    }
}

pub fn bounded_copy(_ctx: &DecodeContext, path: &[u8]) -> Result<Vec<u8>, ()> {
    let path_len = path.len();
    let capacity = path_len.checked_add(1).ok_or_else(|| ())?;
    if path_len >= decode::budget::WorkBudget::INDEPENDENT_RECURSION_DEPTH {
        return Err(());
    }
    let mut copied = Vec::new();
    copied.try_reserve_exact(capacity).map_err(|_| ())?;
    copied.extend(path.iter().copied());
    Ok(copied)
}

pub fn wrong_slice(_ctx: &DecodeContext, path: &[u8], other: &[u8]) -> Result<Vec<u8>, ()> {
    let path_len = path.len();
    let capacity = path_len.checked_add(1).ok_or_else(|| ())?;
    if path_len >= decode::budget::WorkBudget::INDEPENDENT_RECURSION_DEPTH {
        return Err(());
    }
    let mut copied = Vec::new();
    copied.try_reserve_exact(capacity).map_err(|_| ())?;
    copied.extend(other.iter().copied()); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(copied)
}

pub fn guard_after_reserve(_ctx: &DecodeContext, path: &[u8]) -> Result<Vec<u8>, ()> {
    let path_len = path.len();
    let capacity = path_len.checked_add(1).ok_or_else(|| ())?;
    let mut copied = Vec::new();
    copied.try_reserve_exact(capacity).map_err(|_| ())?; // finding: unproven_decode_charge
    if path_len >= decode::budget::WorkBudget::INDEPENDENT_RECURSION_DEPTH {
        return Err(());
    }
    copied.extend(path.iter().copied()); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(copied)
}

pub fn conditional_guard(_ctx: &DecodeContext, path: &[u8], reject: bool) -> Result<Vec<u8>, ()> {
    let path_len = path.len();
    let capacity = path_len.checked_add(1).ok_or_else(|| ())?;
    if reject && path_len >= decode::budget::WorkBudget::INDEPENDENT_RECURSION_DEPTH {
        return Err(());
    }
    let mut copied = Vec::new();
    copied.try_reserve_exact(capacity).map_err(|_| ())?; // finding: unproven_decode_charge
    copied.extend(path.iter().copied()); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(copied)
}

pub fn changed_length_snapshot(_ctx: &DecodeContext, path: &[u8]) -> Result<Vec<u8>, ()> {
    let mut path_len = path.len();
    let capacity = path_len.checked_add(1).ok_or_else(|| ())?;
    if path_len >= decode::budget::WorkBudget::INDEPENDENT_RECURSION_DEPTH {
        return Err(());
    }
    path_len = 0;
    let mut copied = Vec::new();
    copied.try_reserve_exact(capacity).map_err(|_| ())?; // finding: unproven_decode_charge
    copied.extend(path.iter().copied()); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(copied)
}

struct SliceAdapter<'a>(&'a [u8]);

impl<'a> SliceAdapter<'a> {
    fn iter(&self) -> std::slice::Iter<'a, u8> {
        self.0.iter()
    }
}

pub fn custom_iterator(_ctx: &DecodeContext, path: &[u8]) -> Result<Vec<u8>, ()> {
    let path_len = path.len();
    let capacity = path_len.checked_add(1).ok_or_else(|| ())?;
    if path_len >= decode::budget::WorkBudget::INDEPENDENT_RECURSION_DEPTH {
        return Err(());
    }
    let source = SliceAdapter(path);
    let mut copied = Vec::new();
    copied.try_reserve_exact(capacity).map_err(|_| ())?;
    copied.extend(source.iter().copied()); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(copied)
}

pub fn callback_adapter(_ctx: &DecodeContext, path: &[u8]) -> Result<Vec<u8>, ()> {
    let path_len = path.len();
    let capacity = path_len.checked_add(1).ok_or_else(|| ())?;
    if path_len >= decode::budget::WorkBudget::INDEPENDENT_RECURSION_DEPTH {
        return Err(());
    }
    let mut copied = Vec::new();
    copied.try_reserve_exact(capacity).map_err(|_| ())?;
    copied.extend(path.iter().copied().map(|value| value)); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(copied)
}

pub fn old_target(_ctx: &DecodeContext, path: &[u8], copied: &mut Vec<u8>) -> Result<(), ()> {
    let path_len = path.len();
    let capacity = path_len.checked_add(1).ok_or_else(|| ())?;
    if path_len >= decode::budget::WorkBudget::INDEPENDENT_RECURSION_DEPTH {
        return Err(());
    }
    copied.try_reserve_exact(capacity).map_err(|_| ())?; // finding: uncharged_decode_allocation
    copied.extend(path.iter().copied()); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(())
}

pub fn unpropagated_reserve(_ctx: &DecodeContext, path: &[u8]) -> Result<Vec<u8>, ()> {
    let path_len = path.len();
    let capacity = path_len.checked_add(1).ok_or_else(|| ())?;
    if path_len >= decode::budget::WorkBudget::INDEPENDENT_RECURSION_DEPTH {
        return Err(());
    }
    let mut copied = Vec::new();
    if copied.try_reserve_exact(capacity).is_err() {
        // finding: unproven_decode_charge
        return Err(());
    }
    copied.extend(path.iter().copied()); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(copied)
}

pub fn attached_direct_budget(
    _ctx: &DecodeContext,
    session: &decode::budget::DecodeBudget,
    path: &[u8],
) -> Result<Vec<u8>, ()> {
    let path_len = path.len();
    let capacity = path_len.checked_add(1).ok_or_else(|| ())?;
    let capacity_units = u64::try_from(capacity).map_err(|_| ())?;
    let path_units = u64::try_from(path_len).map_err(|_| ())?;
    let element_size = u64::try_from(std::mem::size_of::<u8>()).map_err(|_| ())?;
    let bytes = capacity_units.checked_mul(element_size).ok_or_else(|| ())?;
    let work = bytes.checked_add(path_units).ok_or_else(|| ())?;
    session.charge_collection_items_limit(capacity_units, "bounded slice copy")?;
    session.charge_work_limit(work, "bounded slice copy")?;
    let _storage = session.reserve_scoped_limit(bytes, "bounded slice copy")?;
    let mut copied = Vec::new();
    copied.try_reserve_exact(capacity).map_err(|_| ())?;
    copied.extend(path.iter().copied());
    Ok(copied)
}

pub fn attached_generic_guard_is_transferred_after_growth<T: Copy>(
    _ctx: &DecodeContext,
    session: &decode::budget::DecodeBudget,
    path: &[T],
) -> Result<(Vec<T>, decode::work_scratch::WorkScratch), ()> {
    let path_len = path.len();
    let capacity = path_len.checked_add(1).ok_or_else(|| ())?;
    let capacity_units = u64::try_from(capacity).map_err(|_| ())?;
    let path_units = u64::try_from(path_len).map_err(|_| ())?;
    let element_size = u64::try_from(std::mem::size_of::<T>()).map_err(|_| ())?;
    let bytes = capacity_units.checked_mul(element_size).ok_or_else(|| ())?;
    let work = bytes.checked_add(path_units).ok_or_else(|| ())?;
    session.charge_collection_items_limit(capacity_units, "attached generic copy")?;
    session.charge_work_limit(work, "attached generic copy")?;
    let reservation = session.reserve_scoped_limit(bytes, "attached generic copy")?;
    let mut copied = Vec::new();
    copied.try_reserve_exact(capacity).map_err(|_| ())?;
    copied.extend(path.iter().copied());
    let storage = decode::work_scratch::WorkScratch::from_reservation(reservation);
    Ok((copied, storage))
}

pub fn moved_generic_guard_does_not_prove_growth<T: Copy>(
    _ctx: &DecodeContext,
    session: &decode::budget::DecodeBudget,
    path: &[T],
) -> Result<(Vec<T>, decode::work_scratch::WorkScratch), ()> {
    let path_len = path.len();
    let capacity = path_len.checked_add(1).ok_or_else(|| ())?;
    let capacity_units = u64::try_from(capacity).map_err(|_| ())?;
    let path_units = u64::try_from(path_len).map_err(|_| ())?;
    let element_size = u64::try_from(std::mem::size_of::<T>()).map_err(|_| ())?;
    let bytes = capacity_units.checked_mul(element_size).ok_or_else(|| ())?;
    let work = bytes.checked_add(path_units).ok_or_else(|| ())?;
    session.charge_collection_items_limit(capacity_units, "moved generic guard")?;
    session.charge_work_limit(work, "moved generic guard")?;
    let reservation = session.reserve_scoped_limit(bytes, "moved generic guard")?;
    let storage = decode::work_scratch::WorkScratch::from_reservation(reservation);
    let mut copied = Vec::new();
    copied.try_reserve_exact(capacity).map_err(|_| ())?; // finding: unproven_decode_charge
    copied.extend(path.iter().copied()); // finding: unproven_decode_charge
    Ok((copied, storage))
}

pub fn optional_budget_is_not_authority(
    _ctx: &DecodeContext,
    session: &Option<&decode::budget::DecodeBudget>,
    path: &[u8],
) -> Result<Vec<u8>, ()> {
    let path_len = path.len();
    let capacity = u64::try_from(path_len).map_err(|_| ())?;
    session.charge_work_limit(capacity, "optional slice copy")?;
    let _storage = session.reserve_scoped_limit(capacity, "optional slice copy")?;
    let mut copied = Vec::new();
    copied.try_reserve_exact(path_len).map_err(|_| ())?; // finding: unproven_decode_charge
    copied.extend(path.iter().copied()); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(copied)
}

pub fn work_budget_is_not_decode_authority(
    _ctx: &DecodeContext,
    budget: &decode::budget::WorkBudget,
    path: &[u8],
) -> Result<Vec<u8>, ()> {
    let path_len = path.len();
    let count = u64::try_from(path_len).map_err(|_| ())?;
    budget.charge_work_limit(count, "optional work slice")?;
    let _reservation = budget.reserve_scoped_limit(count, "optional work slice")?;
    let mut copied = Vec::new();
    copied.try_reserve_exact(path_len).map_err(|_| ())?; // finding: unproven_decode_charge
    copied.extend(path.iter().copied()); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(copied)
}

pub fn foreign_budget_is_not_authority(
    _ctx: &DecodeContext,
    budget: &foreign::DecodeBudget,
    path: &[u8],
) -> Result<Vec<u8>, ()> {
    let path_len = path.len();
    let count = u64::try_from(path_len).map_err(|_| ())?;
    budget.charge_work_limit(count, "foreign slice copy")?;
    let _reservation = budget.reserve_scoped_limit(count, "foreign slice copy")?;
    let mut copied = Vec::new();
    copied.try_reserve_exact(path_len).map_err(|_| ())?; // finding: unproven_decode_charge
    copied.extend(path.iter().copied()); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(copied)
}

pub fn bounded_generic_copy<T: Copy>(_ctx: &DecodeContext, path: &[T]) -> Result<Vec<T>, ()> {
    let path_len = path.len();
    let capacity = path_len.checked_add(1).ok_or_else(|| ())?;
    if path_len >= decode::budget::WorkBudget::INDEPENDENT_RECURSION_DEPTH {
        return Err(());
    }
    let mut copied = Vec::new();
    copied.try_reserve_exact(capacity).map_err(|_| ())?;
    copied.extend(path.iter().copied());
    Ok(copied)
}

pub fn conditional_reserve(
    _ctx: &DecodeContext,
    path: &[u8],
    reserve: bool,
) -> Result<Vec<u8>, ()> {
    let path_len = path.len();
    let capacity = path_len.checked_add(1).ok_or_else(|| ())?;
    if path_len >= decode::budget::WorkBudget::INDEPENDENT_RECURSION_DEPTH {
        return Err(());
    }
    let mut copied = Vec::new();
    if reserve {
        copied.try_reserve_exact(capacity).map_err(|_| ())?; // finding: unproven_decode_charge
    }
    copied.extend(path.iter().copied()); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(copied)
}
