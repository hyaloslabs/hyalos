//! The reference skill: the archive writer here and the marketplace's (`tools/pack.py` in
//! hyaloslabs/marketplace) give it the same digest (S7.1, ADR-0094).
//!
//! The marketplace builds the archives Hyalos installs, with a packer of its own, since its pipeline
//! cannot read this private repository. Both pin this digest over the same fixed skill, so a change
//! to either writer fails its own tests before a release could carry a digest an install refuses.

use std::path::Path;

use hyalos_plugin::skill::Skill;

/// Also `REFERENCE_DIGEST` in the marketplace's `tests/test_pack.py`. Never change one alone.
const REFERENCE_DIGEST: &str = "0d085c2a398d33d47f0d1e7143cd460e5bad558cf5b77a9137147a8e6f66f429";

#[test]
fn the_reference_skill_has_the_digest_the_marketplace_pins()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/reference/hyalos-reference");
    let skill = Skill::read(&dir)?;
    assert_eq!(skill.name.as_str(), "hyalos-reference");
    assert_eq!(skill.digest().to_string(), REFERENCE_DIGEST);
    // The archive round-trips to the same skill, as an install reads it.
    let again = Skill::from_archive(&skill.archive())?;
    assert_eq!(again.digest().to_string(), REFERENCE_DIGEST);
    Ok(())
}
