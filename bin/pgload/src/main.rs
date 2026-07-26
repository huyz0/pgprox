//! The load client binary.
//!
//! Deliberately almost empty, same rule as `bin/pgprox`: this file is excluded
//! from coverage, so anything it holds is untested by construction.

fn main() -> Result<(), pgload_app::LoadError> {
    pgload_app::run_with(std::env::args().skip(1))
}
