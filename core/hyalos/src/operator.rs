//! The operator's approval key (ADR-0070).
//!
//! Every human decision is signed with this key as well as the engine's, so the engine's key alone
//! cannot make one that `hyalos log verify` accepts. The key is kept in `operator.key` in the config
//! directory, sealed under a passphrase only the operator knows: PBKDF2-HMAC-SHA256 makes a key from
//! the passphrase and ChaCha20-Poly1305 seals the 32-byte seed with it (both from `ring`). The driving
//! agent may be able to read the file, as it may read `engine.key`, and still cannot sign. The public
//! half is `operator.pub`, which verify reads and never takes from the log.

use std::fs;
use std::num::NonZeroU32;
use std::path::Path;

use hyalos_engine::Engine;
use hyalos_log::format;
use hyalos_types::{OperatorKey, OperatorPublic};
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
use ring::pbkdf2;
use zeroize::Zeroizing;

use crate::keys::{self, SeedSource};
use crate::passphrase::{self, Source};
use crate::paths::{self, Env, OPERATOR_KEY_FILE, OPERATOR_PUBLIC_KEY_FILE};
use crate::policy_cmd;

const PREFIX: &str = "hyalos-operator-key-v1";
/// What a new key is sealed with: OWASP's figure for PBKDF2-HMAC-SHA256.
const ITERATIONS: u32 = 600_000;
/// A file asking for fewer is refused, so a weakened file is not accepted, and one asking for more is
/// refused, so a file cannot make the command spin.
const FEWEST: u32 = 100_000;
const MOST: u32 = 10_000_000;
/// The shortest passphrase a new key takes.
pub const MIN_PASSPHRASE: usize = 12;
const FILE_LIMIT: u64 = 1024;

/// Creates `operator.key` and `operator.pub` in `config_dir`, sealed under `passphrase`. It refuses if
/// either exists: an operator key is never replaced, since the log's signed decisions name it.
pub fn create(
    config_dir: &Path,
    passphrase: &str,
    seeds: &mut dyn SeedSource,
) -> Result<OperatorPublic, String> {
    if passphrase.chars().count() < MIN_PASSPHRASE {
        return Err(format!(
            "a passphrase is at least {MIN_PASSPHRASE} characters"
        ));
    }
    keys::private_dir(config_dir).map_err(|e| e.to_string())?;
    for name in [OPERATOR_KEY_FILE, OPERATOR_PUBLIC_KEY_FILE] {
        let path = config_dir.join(name);
        if fs::symlink_metadata(&path).is_ok() {
            return Err(format!(
                "{} exists: an operator key is never replaced",
                path.display()
            ));
        }
    }
    let mut seed = Zeroizing::new([0u8; 32]);
    seeds
        .fill(&mut seed)
        .map_err(|e| format!("no randomness: {e}"))?;
    let mut extra = Zeroizing::new([0u8; 32]);
    seeds
        .fill(&mut extra)
        .map_err(|e| format!("no randomness: {e}"))?;
    let (salt, nonce) = (&extra[..16], &extra[16..28]);
    let public = OperatorKey::from_seed(*seed).public();
    let mut sealed = seed.to_vec();
    cipher(passphrase, salt, ITERATIONS)?
        .seal_in_place_append_tag(nonce_of(nonce)?, aad(&public), &mut sealed)
        .map_err(|_| "cannot seal the operator key".to_string())?;
    let line = Zeroizing::new(format!(
        "{PREFIX}:{ITERATIONS}:{}:{}:{}:{}\n",
        format::to_hex(salt),
        format::to_hex(nonce),
        public.to_hex(),
        format::to_hex(&sealed)
    ));
    keys::publish_new(config_dir, OPERATOR_KEY_FILE, line.as_bytes(), 0o600)
        .map_err(|e| e.to_string())?;
    keys::publish_new(
        config_dir,
        OPERATOR_PUBLIC_KEY_FILE,
        format!("{}\n", public.to_hex()).as_bytes(),
        0o444,
    )
    .map_err(|e| e.to_string())?;
    Ok(public)
}

/// The key in `path`, an `operator.pub`.
pub fn read_public(path: &Path) -> Result<OperatorPublic, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    OperatorPublic::from_hex(&text)
        .ok_or_else(|| format!("{} is not an operator public key", path.display()))
}

/// The operator's key from `config_dir`, opened with `passphrase`. It must match `operator.pub`.
pub fn unlock(config_dir: &Path, passphrase: &str) -> Result<OperatorKey, String> {
    let sealed = Sealed::read(config_dir)?;
    let public = read_public(&config_dir.join(OPERATOR_PUBLIC_KEY_FILE))?;
    let path = config_dir.join(OPERATOR_KEY_FILE);
    if sealed.public != public {
        return Err(format!(
            "{} is not the key {OPERATOR_PUBLIC_KEY_FILE} names",
            path.display()
        ));
    }
    let mut bytes = Zeroizing::new(sealed.sealed.clone());
    let seed = cipher(passphrase, &sealed.salt, sealed.iterations)?
        .open_in_place(nonce_of(&sealed.nonce)?, aad(&public), &mut bytes)
        .map_err(|_| format!("the passphrase does not open {}", path.display()))?;
    let seed: [u8; 32] = seed
        .try_into()
        .map_err(|_| format!("{} is not an operator key", path.display()))?;
    let key = OperatorKey::from_seed(seed);
    if key.public() != public {
        return Err(format!(
            "{} is not the key {OPERATOR_PUBLIC_KEY_FILE} names",
            path.display()
        ));
    }
    Ok(key)
}

/// The engine on the operator's log with the operator's key, for a command that records a human
/// decision. The key file is checked before the passphrase is asked for.
pub fn engine(env: Env<'_>, source: Source) -> Result<Engine, String> {
    let config_dir = paths::config_dir(env)?;
    Sealed::read(&config_dir)?;
    let passphrase = passphrase::read(source, "operator passphrase: ")?;
    let key = unlock(&config_dir, &passphrase)?;
    Ok(policy_cmd::open(env)?.with_operator(key))
}

struct Sealed {
    iterations: u32,
    salt: Vec<u8>,
    nonce: Vec<u8>,
    public: OperatorPublic,
    sealed: Vec<u8>,
}

impl Sealed {
    fn read(config_dir: &Path) -> Result<Self, String> {
        let path = config_dir.join(OPERATOR_KEY_FILE);
        if fs::symlink_metadata(&path).is_err() {
            return Err(format!(
                "there is no operator key at {}: create one with hyalos operator init",
                path.display()
            ));
        }
        let raw = keys::read_private(&path, FILE_LIMIT).map_err(|e| e.to_string())?;
        Self::parse(&raw).ok_or_else(|| format!("{} is not an operator key", path.display()))
    }

    fn parse(raw: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(raw).ok()?;
        let text = text.strip_suffix('\n').unwrap_or(text);
        let [prefix, iterations, salt, nonce, public, sealed] =
            text.split(':').collect::<Vec<_>>().try_into().ok()?;
        let iterations: u32 = iterations.parse().ok()?;
        if prefix != PREFIX || !(FEWEST..=MOST).contains(&iterations) {
            return None;
        }
        Some(Self {
            iterations,
            salt: format::from_hex::<16>(salt)?.to_vec(),
            nonce: format::from_hex::<12>(nonce)?.to_vec(),
            public: OperatorPublic::from_hex(public)?,
            sealed: format::from_hex::<48>(sealed)?.to_vec(),
        })
    }
}

fn cipher(passphrase: &str, salt: &[u8], iterations: u32) -> Result<LessSafeKey, String> {
    let rounds = NonZeroU32::new(iterations).ok_or("no iterations")?;
    let mut key = Zeroizing::new([0u8; 32]);
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        rounds,
        salt,
        passphrase.as_bytes(),
        &mut *key,
    );
    let unbound = UnboundKey::new(&CHACHA20_POLY1305, &*key)
        .map_err(|_| "cannot make the sealing key".to_string())?;
    Ok(LessSafeKey::new(unbound))
}

fn nonce_of(bytes: &[u8]) -> Result<Nonce, String> {
    Nonce::try_assume_unique_for_key(bytes).map_err(|_| "a nonce is 12 bytes".to_string())
}

/// The seal is bound to the format and to the public key beside it, so neither can be swapped.
fn aad(public: &OperatorPublic) -> Aad<Vec<u8>> {
    let mut aad = PREFIX.as_bytes().to_vec();
    aad.extend_from_slice(public.as_bytes());
    Aad::from(aad)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    use super::*;

    struct Fixed(u8);

    impl SeedSource for Fixed {
        fn fill(&mut self, seed: &mut [u8; 32]) -> Result<(), String> {
            *seed = [self.0; 32];
            self.0 += 1;
            Ok(())
        }
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hyalos-operator-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        dir
    }

    const WORDS: &str = "correct horse battery";

    #[test]
    fn a_key_opens_only_with_its_passphrase_and_is_never_replaced() {
        let d = dir("roundtrip");
        let public = create(&d, WORDS, &mut Fixed(1)).unwrap();
        assert_eq!(public, OperatorKey::from_seed([1; 32]).public());
        assert_eq!(unlock(&d, WORDS).unwrap().public(), public);
        let wrong = unlock(&d, "correct horse battery!").unwrap_err();
        assert!(wrong.contains("does not open"), "{wrong}");
        assert!(
            create(&d, WORDS, &mut Fixed(5))
                .unwrap_err()
                .contains("never replaced")
        );
        // The seed is not in the file in the clear.
        let file = fs::read_to_string(d.join(OPERATOR_KEY_FILE)).unwrap();
        assert!(!file.contains(&format::to_hex(&[1; 32])));
        let mode = fs::metadata(d.join(OPERATOR_KEY_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_short_passphrase_is_refused_and_nothing_is_written() {
        let d = dir("short");
        assert!(create(&d, "too short", &mut Fixed(1)).is_err());
        assert!(!d.join(OPERATOR_KEY_FILE).exists());
    }

    #[test]
    fn a_tampered_or_swapped_file_does_not_open() {
        let d = dir("tamper");
        create(&d, WORDS, &mut Fixed(1)).unwrap();
        let path = d.join(OPERATOR_KEY_FILE);
        let good = fs::read_to_string(&path).unwrap();
        let rewrite = |text: &str| {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            fs::write(&path, text).unwrap();
        };
        // Another public key beside the same sealed seed: refused before the passphrase is tried.
        let other = OperatorKey::from_seed([9; 32]).public().to_hex();
        let fields: Vec<&str> = good.trim_end().split(':').collect();
        rewrite(&good.replace(fields[4], &other));
        assert!(unlock(&d, WORDS).unwrap_err().contains("not the key"));
        // Fewer iterations than a key is ever sealed with.
        rewrite(&good.replacen(":600000:", ":1000:", 1));
        assert!(
            unlock(&d, WORDS)
                .unwrap_err()
                .contains("not an operator key")
        );
        // A flipped byte in the seal.
        let last = fields[5];
        let flipped = format!(
            "{}{}",
            &last[..last.len() - 1],
            if last.ends_with('0') { '1' } else { '0' }
        );
        rewrite(&good.replace(last, &flipped));
        assert!(unlock(&d, WORDS).unwrap_err().contains("does not open"));
        // A key file open to others is refused as engine.key is.
        rewrite(&good);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(unlock(&d, WORDS).is_err());
    }
}
