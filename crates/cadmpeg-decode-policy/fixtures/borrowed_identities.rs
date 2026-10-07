// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub mod index {
    pub mod identities {
        use cadmpeg_core::decode::{u64_from_index, DecodeContext};
        use cadmpeg_core::CodecError;

        pub struct BorrowedIdentities;

        impl BorrowedIdentities {
            pub fn build<'ir, T>(
                ctx: &DecodeContext<'_>,
                visit: impl FnOnce(
                    &mut dyn FnMut(&'ir str, T) -> Result<(), CodecError>,
                ) -> Result<(), CodecError>,
            ) -> Result<(), CodecError> {
                let mut storage = ctx.reserve_scoped(0, "borrowed validation identities")?;
                let mut values = Vec::new();
                visit(&mut |id, value| {
                    ctx.charge_work(1, "validation identity record scan")?;
                    ctx.charge_work(u64_from_index(id.len()), "hash validation identity")?;
                    let hash = u64_from_index(id.len());
                    storage.with_storage(|| {
                        ctx.push_vec(
                            &mut values,
                            (hash, id, value),
                            "borrowed validation identity slots",
                        )
                    })
                })?;
                Ok(())
            }
        }
    }
}

pub mod other_identities {
    use super::DecodeContext;
    use cadmpeg_core::CodecError;

    pub struct BorrowedIdentities;

    impl BorrowedIdentities {
        pub fn build<'ir, T>(
            _ctx: &DecodeContext<'_>,
            visit: &mut dyn FnMut(
                &mut dyn FnMut(&'ir str, T) -> Result<(), CodecError>,
            ) -> Result<(), CodecError>,
        ) -> Result<(), CodecError> {
            visit(&mut |id, value| { // finding: unproven_decode_charge
                let _ = (id, value);
                Ok(())
            })?;
            Ok(())
        }
    }
}

pub fn charged(ctx: &DecodeContext<'_>, id: &str) -> Result<(), CodecError> {
    index::identities::BorrowedIdentities::build::<()>(ctx, |add| {
        add(id, ())?;
        Ok(())
    })?;
    Ok(())
}

pub fn captured(
    ctx: &DecodeContext<'_>,
    id: &str,
    callback: &mut dyn FnMut(&str, ()) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    index::identities::BorrowedIdentities::build::<()>(ctx, |_add| {
        callback(id, ())?; // finding: unproven_decode_charge
        Ok(())
    })?;
    Ok(())
}

pub fn swallowed(ctx: &DecodeContext<'_>, id: &str) -> Result<(), CodecError> {
    index::identities::BorrowedIdentities::build::<()>(ctx, |add| {
        let _ = add(id, ()); // finding: unproven_decode_charge
        Ok(())
    })?;
    Ok(())
}

pub fn uncharged_path(ctx: &DecodeContext<'_>, id: &str) -> Result<(), CodecError> {
    other_identities::BorrowedIdentities::build::<()>(ctx, &mut |add| {
        add(id, ())?; // finding: unproven_decode_charge
        Ok(())
    })?;
    Ok(())
}
