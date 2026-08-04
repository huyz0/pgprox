//! A hasher for map keys this process issues.
//!
//! # The rule
//!
//! **Who chooses the key decides which hasher it gets.** A key a peer chooses
//! keeps `RandomState`, which is `SipHash` with a per-process seed, because that
//! is what stops a client filling one bucket with a thousand collisions and
//! turning a lookup into a scan. A key this process hands out gets [`IssuedIds`]
//! instead, because there is nobody to defend against and `SipHash` is not free.
//!
//! It is a rule about the key and not about the map. The same map type appears
//! on both sides of it:
//!
//! | Key | Hasher | Why |
//! | --- | --- | --- |
//! | [`crate::pool::UpstreamId`] | [`IssuedIds`] | a `u64` counter this node increments |
//! | `CacheKey` | `RandomState` | holds the client's SQL, database and role |
//! | `PoolKey` | `RandomState` | holds the database and user a grant resolved to |
//! | a prepared statement's global name | `RandomState` | derived from a name the client chose |
//!
//! The third and fourth are the ones worth reading twice. Both are values this
//! process computes, and neither is a value this process *chooses*: a client
//! that picks its own statement names picks what goes into the hash, and a hash
//! of peer input is peer input.
//!
//! # What it is worth
//!
//! `SipHash` over an `UpstreamId` was 174 instructions of `acquire_and_release`'s
//! 443, which is 39% of the pool's per-statement cost spent defending an
//! integer this node made up. `M30.3`.
//!
//! # What this is not
//!
//! Not a general-purpose fast hasher, and not something to reach for because a
//! map looked slow in a profile. The argument here is entirely about who
//! supplies the key, and a map that fails that test does not get this however
//! much it would gain.

use std::hash::{BuildHasherDefault, Hasher};

/// The hasher [`IssuedIds`] builds.
///
/// Deterministic and unseeded, which is the whole point and is also why it must
/// never see a key a peer picked: two keys that collide here collide on every
/// node and in every process, forever.
#[derive(Default, Clone, Copy, Debug)]
pub struct IssuedIdHasher(u64);

/// [`std::hash::BuildHasher`] for a map keyed on something this process issued.
///
/// ```
/// use std::collections::HashMap;
///
/// use pgprox_core::hash::IssuedIds;
/// use pgprox_core::pool::UpstreamId;
///
/// let mut open: HashMap<UpstreamId, &str, IssuedIds> = HashMap::default();
/// open.insert(UpstreamId(7), "primary");
///
/// assert_eq!(open.get(&UpstreamId(7)), Some(&"primary"));
/// assert_eq!(open.get(&UpstreamId(8)), None);
/// ```
pub type IssuedIds = BuildHasherDefault<IssuedIdHasher>;

/// Sebastiano Vigna's `splitmix64` finalizer.
///
/// Two multiplies and three xor-shifts, and every output bit depends on every
/// input bit. That last part is the requirement rather than a nicety: a
/// `HashMap` takes the bucket from the low bits and its control byte from the
/// top seven, so a mixer that only scrambles one end puts every key with the
/// same low bits in the same bucket.
///
/// It is not a keyed hash and does not pretend to be. See the module docs for
/// the one condition under which that is acceptable.
const fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

impl Hasher for IssuedIdHasher {
    fn finish(&self) -> u64 {
        mix(self.0)
    }

    /// The path a `&str` or a `&[u8]` key takes.
    ///
    /// Correct rather than tuned. Nothing this hasher is for hashes bytes, and
    /// a key that does should be asking whether it belongs here at all, so this
    /// exists to make the implementation total rather than to be fast.
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            let mut word = [0_u8; 8];
            word.copy_from_slice(chunk);
            self.write_u64(u64::from_ne_bytes(word));
        }

        let mut tail = [0_u8; 8];
        let rest = chunks.remainder();
        tail[..rest.len()].copy_from_slice(rest);
        // The length goes in, so `[1]` and `[1, 0]` are different keys.
        self.write_u64(u64::from_ne_bytes(tail) ^ bytes.len() as u64);
    }

    /// Where every key this hasher exists for arrives.
    ///
    /// The mixing happens in [`IssuedIdHasher::finish`] rather than here, so a
    /// single-field key costs one rotate, one xor and one multiply on the way
    /// in and the finalizer once on the way out.
    fn write_u64(&mut self, n: u64) {
        // Rotate before combining so two fields written in the other order do
        // not produce the same state.
        self.0 = (self.0.rotate_left(17) ^ n).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn write_u32(&mut self, n: u32) {
        self.write_u64(u64::from(n));
    }

    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }

    fn write_u8(&mut self, n: u8) {
        self.write_u64(u64::from(n));
    }

    fn write_u16(&mut self, n: u16) {
        self.write_u64(u64::from(n));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::hash::{BuildHasher, Hash, Hasher};

    use super::{IssuedIdHasher, IssuedIds, mix};

    fn hash_of<T: Hash + ?Sized>(value: &T) -> u64 {
        IssuedIds::default().hash_one(value)
    }

    #[test]
    fn a_map_keyed_on_an_issued_id_finds_what_it_stored() {
        let mut map: HashMap<u64, &str, IssuedIds> = HashMap::default();
        for i in 0..1_000_u64 {
            map.insert(i, "open");
        }

        assert_eq!(map.len(), 1_000);
        for i in 0..1_000_u64 {
            assert_eq!(map.get(&i), Some(&"open"), "{i} went missing");
        }
        assert_eq!(map.get(&1_000), None);
    }

    #[test]
    fn consecutive_ids_do_not_collide() {
        // The case this hasher exists for and the one an unmixed hasher gets
        // wrong: ids come off a counter, so they arrive dense and in order. A
        // `HashMap` takes its bucket from the low bits, and identity would put
        // every one of these in a different bucket by luck rather than by
        // mixing. This asserts the hash values themselves are distinct, which
        // is the property the map then relies on.
        let hashes: HashSet<u64> = (0..10_000_u64).map(|i| hash_of(&i)).collect();
        assert_eq!(hashes.len(), 10_000, "two ids off one counter collided");
    }

    #[test]
    fn every_output_bit_moves() {
        // A mixer that leaves the top bits alone breaks a `HashMap` quietly: the
        // control byte comes from the top seven, so the map would keep working
        // and every lookup would compare more keys than it should.
        //
        // Flip one input bit at a time and record which output bits changed.
        // Over 64 single-bit flips every output bit should have moved at least
        // once, and for a good mixer roughly half move on each flip.
        let mut ever_moved = 0_u64;
        for bit in 0..64 {
            let changed = mix(0) ^ mix(1 << bit);
            ever_moved |= changed;
            assert!(
                changed.count_ones() > 8,
                "flipping bit {bit} moved only {} output bits",
                changed.count_ones()
            );
        }
        assert_eq!(ever_moved, u64::MAX, "some output bit never moved");
    }

    #[test]
    fn the_whole_hasher_avalanches_and_not_just_its_finalizer() {
        // What a map sees is `write_u64` and then `finish`, and the test above
        // covers only the second of those. That gap was found by mutation:
        // deleting `mix` from `finish` entirely left every other test in this
        // file passing, because `write_u64`'s multiply is a bijection, so ids
        // still landed in distinct buckets and the map still worked. It was a
        // real weakening and nothing said so. The top bits become one bit of a
        // shifted constant, and the top seven are the control byte a `HashMap`
        // compares before it compares a key.
        for bit in 0..64_u32 {
            let changed = hash_of(&0_u64) ^ hash_of(&(1_u64 << bit));
            assert!(
                changed.count_ones() > 8,
                "flipping input bit {bit} moved only {} bits of the hash",
                changed.count_ones()
            );
        }
    }

    #[test]
    fn field_order_changes_the_hash() {
        // A key with two fields is hashed as two writes, and a hasher that
        // simply xored them would give `(a, b)` and `(b, a)` the same hash.
        assert_ne!(hash_of(&(1_u64, 2_u64)), hash_of(&(2_u64, 1_u64)));
    }

    #[test]
    fn every_field_reaches_the_hash() {
        // The test above passes for a hasher that keeps only the field it saw
        // last, because two fields swapped still leave a different one last.
        // Found by mutation: replacing the whole of `write_u64` with an
        // assignment failed nothing here.
        //
        // A key whose first field is ignored is a key that is not the key, and
        // for a two-field map it means half the keyspace collapsing into one
        // bucket per second field.
        assert_ne!(hash_of(&(1_u64, 2_u64)), hash_of(&(5_u64, 2_u64)));
        assert_ne!(
            hash_of(&(7_u64, 7_u64, 1_u64)),
            hash_of(&(9_u64, 7_u64, 1_u64))
        );
    }

    #[test]
    fn a_byte_key_is_hashed_by_length_as_well_as_content() {
        // The generic path is not what this hasher is for, and it still has to
        // be a hash: without the length, a trailing zero byte would be free.
        //
        // Through `Hasher::write` directly rather than through `Hash`, which is
        // the only way this says anything. `impl Hash for [T]` writes the length
        // before the bytes and `impl Hash for str` writes a terminator after
        // them, so both already disambiguate and a version of `write` that
        // ignored the length passed this test when it went through them. Found
        // by mutation, like the two above.
        fn written(bytes: &[u8]) -> u64 {
            let mut hasher = IssuedIdHasher::default();
            hasher.write(bytes);
            hasher.finish()
        }

        assert_ne!(written(&[1]), written(&[1, 0]));
        assert_ne!(written(&[0; 8]), written(&[0; 16]));
        assert_eq!(written(&[3, 4]), written(&[3, 4]));

        // And the whole-key path still separates lengths, whichever of the two
        // is doing it.
        assert_ne!(hash_of(&[0_u8; 9][..]), hash_of(&[0_u8; 17][..]));
    }

    #[test]
    fn the_hasher_is_deterministic_across_instances() {
        // Unseeded on purpose, and worth asserting rather than assuming: it is
        // what makes this unsuitable for a key a peer chooses, and a later
        // change that quietly seeded it would make the module docs wrong
        // without making anything fail.
        assert_eq!(
            IssuedIds::default().hash_one(42_u64),
            IssuedIds::default().hash_one(42_u64)
        );
        assert_eq!(IssuedIdHasher::default().0, 0);
    }
}
