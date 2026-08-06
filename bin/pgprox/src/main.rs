//! The proxy binary.
//!
//! Deliberately almost empty. This file is the only one excluded from
//! coverage, so anything it holds is untested by construction, and
//! `scripts/gates/m6-complete.sh` fails if it grows past a handful of lines.
//!
//! Returning the error rather than printing it is what keeps the exclusion
//! honest: the formatting is `StartupError`'s, and that is tested.
//!
//! The logging install is here rather than in `entry::serve` because it is
//! process-wide and `main` is the only thing that owns the process. `M19.7`:
//! with it inside `serve`, the test that exercises `run_with` installed the
//! subscriber too, and it and `logging::tests::installing_twice_is_not_a_panic`
//! raced for the one install a process gets. Earlier than before, so an
//! argument this binary rejects is rejected with logging already up.

fn main() -> Result<(), pgprox_app::StartupError> {
    pgprox_app::logging::init();
    pgprox_app::run_with(std::env::args().skip(1))
}
