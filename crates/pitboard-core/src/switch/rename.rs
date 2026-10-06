//! Changing the label an account is enrolled under.

use super::{Result, Settled};
use crate::state::{self, Key};

/// Returns the account's email.
pub fn rename(settled: Settled, from: &Key, to: &str) -> Result<String> {
    let Settled {
        _exclusive,
        mut state,
        ctx,
        permit,
    } = settled;
    let email = state.relabel(from, to)?.email.clone();
    state::save(&ctx, permit, &state)?;
    Ok(email)
}
