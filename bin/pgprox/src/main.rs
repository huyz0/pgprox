//! The proxy binary.
//!
//! Deliberately almost empty. This file is the only one excluded from
//! coverage, so anything it holds is untested by construction, and
//! `scripts/m6-complete.sh` fails if it grows past a handful of lines.
//!
//! Returning the error rather than printing it is what keeps the exclusion
//! honest: the formatting is `StartupError`'s, and that is tested.

fn main() -> Result<(), pgprox_app::StartupError> {
    pgprox_app::run_with(std::env::args().skip(1))
}
