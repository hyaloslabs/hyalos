//! The engine's log signing key on disk (ADR-0022).
//!
//! The key is generated once from operating-system entropy, here and not in a deterministic crate, and
//! kept in `engine.key` (mode 0600) in the config directory (mode 0700). The public half goes to
//! `engine.pub` (mode 0444), which `hyalos log verify` reads. The functions here decide when a key may
//! be created and when the engine must refuse, so that a log can never quietly change keys.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use hyalos_log::{Log, LogError, LogKey, PublicKey, format};
use zeroize::Zeroizing;

use crate::paths::{ENGINE_KEY_FILE, ENGINE_PUBLIC_KEY_FILE};

const KEY_PREFIX: &str = "hyalos-engine-key-v1:";
/// A key file is one short line. Reading stops here, so a wrong file cannot be pulled into memory whole.
const KEY_FILE_LIMIT: u64 = 1024;

/// Where a new key's 32 bytes of randomness come from.
pub trait SeedSource {
    fn fill(&mut self, seed: &mut [u8; 32]) -> Result<(), String>;
}

/// The operating system's entropy source.
pub struct OsSeed;

impl SeedSource for OsSeed {
    fn fill(&mut self, seed: &mut [u8; 32]) -> Result<(), String> {
        getrandom::fill(seed).map_err(|e| e.to_string())
    }
}

/// Why the engine's key could not be made ready. No variant carries key material.
#[derive(Debug)]
pub enum KeyError {
    Io {
        what: String,
        error: String,
    },
    Entropy(String),
    /// The config directory can be read or written by group or others.
    DirAccessible {
        path: PathBuf,
        mode: u32,
    },
    /// The config directory is owned by a user other than the one running the engine.
    DirNotOwned {
        path: PathBuf,
        owner: u32,
        user: u32,
    },
    /// The key file is owned by a user other than the one running the engine.
    KeyNotOwned {
        path: PathBuf,
        owner: u32,
        user: u32,
    },
    /// The key file can be read or written by group or others.
    KeyAccessible {
        path: PathBuf,
        mode: u32,
    },
    /// The key path is a symlink, a directory or something else that is not a regular file.
    NotRegular(PathBuf),
    /// The file is not a Hyalos engine key.
    Malformed(PathBuf),
    /// A log exists and its key does not.
    LostKey {
        key: PathBuf,
        log: PathBuf,
        public: PathBuf,
    },
    /// `engine.pub` exists with no key and no log: it belongs to an earlier log.
    StalePublic(PathBuf),
    /// `engine.pub` does not match `engine.key`.
    PublicMismatch {
        public: PathBuf,
        key: PathBuf,
    },
    /// The key is not the one the log was made with.
    WrongKey {
        key: PathBuf,
        log: PathBuf,
    },
    /// A temporary file from an interrupted run is in the way.
    StaleTemporary(PathBuf),
    /// Another process is creating the same file.
    Busy(PathBuf),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyError::Io { what, error } => write!(f, "cannot {what}: {error}"),
            KeyError::Entropy(e) => {
                write!(f, "cannot get random bytes from the operating system: {e}")
            }
            KeyError::DirAccessible { path, mode } => write!(
                f,
                "{} can be used by other users (mode {:04o}), so a key kept there cannot be trusted; run chmod 700 on it",
                path.display(),
                mode & 0o7777
            ),
            KeyError::DirNotOwned { path, owner, user } => write!(
                f,
                "{} is owned by user id {owner}, but hyalos is running as user id {user}, so a key kept there cannot be trusted. \
                 If the directory is yours, run `chown {user} {}`; if you did not put it there, treat its contents as compromised (ADR-0022)",
                path.display(),
                path.display()
            ),
            KeyError::KeyNotOwned { path, owner, user } => write!(
                f,
                "{} is owned by user id {owner}, but hyalos is running as user id {user}, so it is not used as the engine's key. \
                 If the key is yours, run `chown {user} {}`; if you did not put it there, treat it as compromised, and if another user \
                 may have read it, treat it as leaked (ADR-0022)",
                path.display(),
                path.display()
            ),
            KeyError::KeyAccessible { path, mode } => write!(
                f,
                "{} can be read or written by other users (mode {:04o}); run chmod 600 on it. If another user may have read it, treat the key as leaked (ADR-0022)",
                path.display(),
                mode & 0o7777
            ),
            KeyError::NotRegular(path) => write!(
                f,
                "{} is a symlink or not a regular file, so it is not used as the engine's key",
                path.display()
            ),
            KeyError::Malformed(path) => write!(
                f,
                "{} is not an engine key file (expected one line starting {KEY_PREFIX} and 64 hexadecimal characters)",
                path.display()
            ),
            KeyError::LostKey { key, log, public } => write!(
                f,
                "the event log {} exists but the engine key {} does not, so the engine will not start a new key beside it.\n\
                 If you have a backup of engine.key, put it at that path with mode 0600 and run hyalos init.\n\
                 If not, that log can no longer be extended, but it still verifies with `hyalos log verify`. To start a new log, \
                 move {} and {} aside together (keep them: they are how the old log is verified), then run hyalos init. \
                 Tasks, jobs and holds do not carry across (ADR-0022).",
                log.display(),
                key.display(),
                log.display(),
                public.display()
            ),
            KeyError::StalePublic(public) => write!(
                f,
                "{} exists but there is no engine key and no log. It verifies an earlier log, so it is not overwritten; move it aside with that log's archive, then run hyalos init",
                public.display()
            ),
            KeyError::PublicMismatch { public, key } => write!(
                f,
                "{} does not match the key in {}. One of them belongs to a different log; nothing was changed",
                public.display(),
                key.display()
            ),
            KeyError::WrongKey { key, log } => write!(
                f,
                "the key in {} is not the key {} was made with, so the engine will not continue that log",
                key.display(),
                log.display()
            ),
            KeyError::StaleTemporary(path) => write!(
                f,
                "{} is left from an interrupted run; check that no other hyalos is starting, then remove it",
                path.display()
            ),
            KeyError::Busy(path) => write!(
                f,
                "{} is being created by another process; try again in a moment",
                path.display()
            ),
        }
    }
}

impl std::error::Error for KeyError {}

fn io(what: impl Into<String>, error: std::io::Error) -> KeyError {
    KeyError::Io {
        what: what.into(),
        error: error.to_string(),
    }
}

/// A key that is ready to use.
pub struct Ready {
    pub key: LogKey,
    pub public: PublicKey,
    /// The key was generated by this call.
    pub created: bool,
    /// `engine.pub` was missing and has been written from the key.
    pub public_rewritten: bool,
}

// Never print the key, even if a debug line one day names this struct.
impl fmt::Debug for Ready {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ready")
            .field("public", &self.public.to_hex())
            .field("created", &self.created)
            .field("public_rewritten", &self.public_rewritten)
            .finish_non_exhaustive()
    }
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn make_dir(path: &Path) -> Result<(), KeyError> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|e| io(format!("create {}", path.display()), e))
}

/// The user id the engine runs as. Files it trusts with its key must belong to this user (ADR-0022).
fn current_user() -> u32 {
    rustix::process::geteuid().as_raw()
}

/// Makes sure the engine's key exists and is safe to use, creating it only when neither a key nor a log
/// exists (ADR-0022). `log` is the path of `events.db`.
pub fn prepare(
    config_dir: &Path,
    log: &Path,
    seeds: &mut dyn SeedSource,
) -> Result<Ready, KeyError> {
    prepare_as(config_dir, log, seeds, current_user())
}

/// [`prepare`] for the user with this id. Tests use it to stand in for a user who is not the file's owner.
fn prepare_as(
    config_dir: &Path,
    log: &Path,
    seeds: &mut dyn SeedSource,
    user: u32,
) -> Result<Ready, KeyError> {
    make_dir(config_dir)?;
    if let Some(data_dir) = log.parent().filter(|d| !d.as_os_str().is_empty()) {
        make_dir(data_dir)?;
    }
    let dir_meta =
        fs::metadata(config_dir).map_err(|e| io(format!("read {}", config_dir.display()), e))?;
    if dir_meta.uid() != user {
        return Err(KeyError::DirNotOwned {
            path: config_dir.to_path_buf(),
            owner: dir_meta.uid(),
            user,
        });
    }
    let dir_mode = dir_meta.mode();
    if dir_mode & 0o077 != 0 {
        return Err(KeyError::DirAccessible {
            path: config_dir.to_path_buf(),
            mode: dir_mode,
        });
    }

    let key_path = config_dir.join(ENGINE_KEY_FILE);
    let public_path = config_dir.join(ENGINE_PUBLIC_KEY_FILE);

    if !exists(&key_path) {
        if exists(log) {
            return Err(KeyError::LostKey {
                key: key_path,
                log: log.to_path_buf(),
                public: public_path,
            });
        }
        if exists(&public_path) {
            return Err(KeyError::StalePublic(public_path));
        }
        return create(config_dir, seeds);
    }

    let key = load_as(&key_path, user)?;
    let public = key.public();
    let public_rewritten = if exists(&public_path) {
        let on_disk = fs::read_to_string(&public_path)
            .map_err(|e| io(format!("read {}", public_path.display()), e))?;
        if PublicKey::from_hex(&on_disk) != Some(public) {
            return Err(KeyError::PublicMismatch {
                public: public_path,
                key: key_path,
            });
        }
        false
    } else {
        publish_new(
            config_dir,
            ENGINE_PUBLIC_KEY_FILE,
            public_text(&public).as_bytes(),
            0o444,
        )?;
        true
    };
    if exists(log) {
        // Opening checks that the chain starts with this key's genesis; it writes nothing to a log that
        // already has events.
        match Log::open(log, load_as(&key_path, user)?) {
            Ok(_) => {}
            Err(LogError::WrongKey) => {
                return Err(KeyError::WrongKey {
                    key: key_path,
                    log: log.to_path_buf(),
                });
            }
            Err(other) => {
                return Err(KeyError::Io {
                    what: format!("open {}", log.display()),
                    error: other.to_string(),
                });
            }
        }
    }
    Ok(Ready {
        key,
        public,
        created: false,
        public_rewritten,
    })
}

fn public_text(public: &PublicKey) -> String {
    format!("{}\n", public.to_hex())
}

fn create(config_dir: &Path, seeds: &mut dyn SeedSource) -> Result<Ready, KeyError> {
    let mut seed = Zeroizing::new([0u8; 32]);
    seeds.fill(&mut seed).map_err(KeyError::Entropy)?;
    let text = Zeroizing::new(format!("{KEY_PREFIX}{}\n", format::to_hex(&*seed)));
    publish_new(config_dir, ENGINE_KEY_FILE, text.as_bytes(), 0o600)?;
    let key = LogKey::from_seed(*seed);
    let public = key.public();
    publish_new(
        config_dir,
        ENGINE_PUBLIC_KEY_FILE,
        public_text(&public).as_bytes(),
        0o444,
    )?;
    Ok(Ready {
        key,
        public,
        created: true,
        public_rewritten: false,
    })
}

/// Creates `dir/name` with `contents` and `mode`, atomically and without ever replacing a file: the
/// bytes are written and flushed to a temporary file made exclusively, then linked into place.
fn publish_new(dir: &Path, name: &str, contents: &[u8], mode: u32) -> Result<(), KeyError> {
    let target = dir.join(name);
    let temp = dir.join(format!("{name}.tmp"));
    let mut file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&temp)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(KeyError::StaleTemporary(temp));
        }
        Err(e) => return Err(io(format!("create {}", temp.display()), e)),
    };
    let written = file.write_all(contents).and_then(|()| file.sync_all());
    drop(file);
    let linked = written.and_then(|()| fs::hard_link(&temp, &target));
    let _ = fs::remove_file(&temp);
    match linked {
        Ok(()) => File::open(dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| io(format!("flush {}", dir.display()), e)),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(KeyError::Busy(target)),
        Err(e) => Err(io(format!("write {}", target.display()), e)),
    }
}

/// Reads the engine key, refusing a file that belongs to another user or is not private to its owner.
pub fn load(path: &Path) -> Result<LogKey, KeyError> {
    load_as(path, current_user())
}

fn load_as(path: &Path, user: u32) -> Result<LogKey, KeyError> {
    let raw = read_private_as(path, KEY_FILE_LIMIT, user)?;
    let seed = parse(&raw).ok_or_else(|| KeyError::Malformed(path.to_path_buf()))?;
    Ok(LogKey::from_seed(*seed))
}

/// Reads a secret file: a regular file owned by the effective user and closed to group and others
/// (ADR-0030), at most `limit` bytes. The provider key file is read this way (ADR-0038).
pub fn read_private(path: &Path, limit: u64) -> Result<Zeroizing<Vec<u8>>, KeyError> {
    read_private_as(path, limit, current_user())
}

fn read_private_as(path: &Path, limit: u64, user: u32) -> Result<Zeroizing<Vec<u8>>, KeyError> {
    let meta = fs::symlink_metadata(path).map_err(|e| io(format!("read {}", path.display()), e))?;
    if !meta.file_type().is_file() {
        return Err(KeyError::NotRegular(path.to_path_buf()));
    }
    if meta.uid() != user {
        return Err(KeyError::KeyNotOwned {
            path: path.to_path_buf(),
            owner: meta.uid(),
            user,
        });
    }
    if meta.mode() & 0o077 != 0 {
        return Err(KeyError::KeyAccessible {
            path: path.to_path_buf(),
            mode: meta.mode(),
        });
    }
    let mut raw = Zeroizing::new(Vec::new());
    File::open(path)
        .and_then(|f| f.take(limit).read_to_end(&mut raw))
        .map_err(|e| io(format!("read {}", path.display()), e))?;
    Ok(raw)
}

fn parse(raw: &[u8]) -> Option<Zeroizing<[u8; 32]>> {
    let text = std::str::from_utf8(raw).ok()?;
    let hex = text
        .strip_suffix('\n')
        .unwrap_or(text)
        .strip_prefix(KEY_PREFIX)?;
    format::from_hex::<32>(hex).map(Zeroizing::new)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use hyalos_engine::Engine;
    use hyalos_log::{Origin, copy_verified, verify};
    use hyalos_types::{Digest, Event, Timestamp};

    use super::*;

    const SEED: [u8; 32] = [0x5a; 32];

    struct Fixed(Option<[u8; 32]>);

    impl SeedSource for Fixed {
        fn fill(&mut self, seed: &mut [u8; 32]) -> Result<(), String> {
            *seed = self.0.take().ok_or("no randomness left")?;
            Ok(())
        }
    }

    struct Failing;

    impl SeedSource for Failing {
        fn fill(&mut self, _: &mut [u8; 32]) -> Result<(), String> {
            Err("no entropy source".into())
        }
    }

    struct Machine {
        config: PathBuf,
        log: PathBuf,
    }

    fn machine(name: &str) -> Machine {
        let root = std::env::temp_dir().join(format!("hyalos-keys-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        Machine {
            config: root.join("config"),
            log: root.join("data").join("events.db"),
        }
    }

    impl Machine {
        fn first_start(&self) -> Ready {
            prepare(&self.config, &self.log, &mut Fixed(Some(SEED))).expect("first start")
        }

        fn key_path(&self) -> PathBuf {
            self.config.join(ENGINE_KEY_FILE)
        }

        fn public_path(&self) -> PathBuf {
            self.config.join(ENGINE_PUBLIC_KEY_FILE)
        }

        fn err(&self) -> KeyError {
            prepare(&self.config, &self.log, &mut Fixed(Some([7; 32]))).expect_err("should refuse")
        }
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).expect("metadata").mode() & 0o777
    }

    fn set_mode(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("chmod");
    }

    fn append_events(log: &Path, key: LogKey, n: u64) {
        let mut engine = Engine::open(log, key).expect("open engine");
        for i in 1..=n {
            engine
                .submit(
                    Timestamp::from_millis(i),
                    Origin::Engine,
                    Event::TaskCreated {
                        request: Digest::from_bytes([i as u8; 32]),
                    },
                )
                .expect("submit");
        }
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn first_start_creates_directories_the_key_and_its_public_half() {
        let m = machine("first");
        let ready = m.first_start();
        assert!(ready.created && !ready.public_rewritten);
        assert_eq!(mode(&m.config), 0o700);
        assert_eq!(mode(m.log.parent().expect("data dir")), 0o700);
        assert_eq!(mode(&m.key_path()), 0o600);
        assert_eq!(mode(&m.public_path()), 0o444);
        assert_eq!(
            fs::read_to_string(m.key_path()).expect("key"),
            format!("hyalos-engine-key-v1:{}\n", "5a".repeat(32))
        );
        assert_eq!(
            fs::read_to_string(m.public_path()).expect("pub"),
            format!("{}\n", LogKey::from_seed(SEED).public().to_hex())
        );
        assert_eq!(ready.public, LogKey::from_seed(SEED).public());
        // Nothing but the two files, and no temporary file.
        assert!(!exists(&m.config.join("engine.key.tmp")));
        assert!(!exists(&m.config.join("engine.pub.tmp")));
    }

    #[test]
    fn a_second_start_loads_the_same_key_and_changes_nothing() {
        let m = machine("second");
        let first = m.first_start();
        let before = (
            fs::read(m.key_path()).expect("key"),
            fs::read(m.public_path()).expect("pub"),
        );
        let again =
            prepare(&m.config, &m.log, &mut Failing).expect("second start needs no entropy");
        assert!(!again.created && !again.public_rewritten);
        assert_eq!(again.public, first.public);
        assert_eq!(
            (
                fs::read(m.key_path()).expect("key"),
                fs::read(m.public_path()).expect("pub")
            ),
            before
        );
    }

    #[test]
    fn a_missing_public_key_is_rewritten_from_the_key() {
        let m = machine("rewrite-pub");
        let first = m.first_start();
        set_mode(&m.public_path(), 0o644);
        fs::remove_file(m.public_path()).expect("remove");
        let again = prepare(&m.config, &m.log, &mut Failing).expect("restart");
        assert!(again.public_rewritten);
        assert_eq!(again.public, first.public);
        assert_eq!(mode(&m.public_path()), 0o444);
    }

    #[test]
    fn a_public_key_that_disagrees_with_the_key_is_refused_and_nothing_changes() {
        let m = machine("pub-mismatch");
        m.first_start();
        set_mode(&m.public_path(), 0o644);
        let other = format!("{}\n", LogKey::from_seed([1; 32]).public().to_hex());
        fs::write(m.public_path(), &other).expect("write");
        assert!(matches!(m.err(), KeyError::PublicMismatch { .. }));
        assert_eq!(fs::read_to_string(m.public_path()).expect("pub"), other);
        assert_eq!(
            fs::read_to_string(m.key_path()).expect("key"),
            format!("hyalos-engine-key-v1:{}\n", "5a".repeat(32))
        );
    }

    #[test]
    fn a_log_without_its_key_is_a_lost_key_and_no_new_key_is_made() {
        let m = machine("lost");
        m.first_start();
        append_events(&m.log, LogKey::from_seed(SEED), 3);
        fs::remove_file(m.key_path()).expect("lose the key");
        let err = m.err();
        assert!(matches!(err, KeyError::LostKey { .. }), "{err}");
        let text = err.to_string();
        assert!(
            text.contains("hyalos init")
                && text.contains("events.db")
                && text.contains("engine.pub"),
            "{text}"
        );
        assert!(
            !exists(&m.key_path()),
            "a new key must not appear beside the old log"
        );
        // The old log still verifies from the public key that was kept.
        let public =
            PublicKey::from_hex(&fs::read_to_string(m.public_path()).expect("pub")).expect("hex");
        assert_eq!(verify(&m.log, &public, None).expect("verifies").events, 3);
    }

    #[test]
    fn starting_a_new_log_after_a_lost_key_means_moving_the_old_log_and_public_key_aside() {
        let m = machine("start-over");
        m.first_start();
        append_events(&m.log, LogKey::from_seed(SEED), 2);
        fs::remove_file(m.key_path()).expect("lose the key");
        // Moving only the log leaves engine.pub, which still verifies the archive, so it is protected.
        fs::rename(&m.log, m.log.with_extension("old")).expect("move log");
        assert!(matches!(m.err(), KeyError::StalePublic(_)));
        set_mode(&m.public_path(), 0o644);
        fs::rename(m.public_path(), m.config.join("engine.pub.old")).expect("move pub");
        let fresh = prepare(&m.config, &m.log, &mut Fixed(Some([9; 32]))).expect("new key");
        assert!(fresh.created);
        assert_ne!(fresh.public, LogKey::from_seed(SEED).public());
    }

    #[test]
    fn a_key_that_is_not_the_logs_key_is_refused() {
        let m = machine("wrong-key");
        m.first_start();
        // A log made by some other key sits where this machine's log should be.
        fs::create_dir_all(m.log.parent().expect("data dir")).expect("dir");
        drop(Log::open(&m.log, LogKey::from_seed([2; 32])).expect("other log"));
        let mut other = Engine::open(&m.log, LogKey::from_seed([2; 32])).expect("open");
        other
            .submit(
                Timestamp::from_millis(1),
                Origin::Engine,
                Event::TaskCreated {
                    request: Digest::from_bytes([1; 32]),
                },
            )
            .expect("submit");
        drop(other);
        assert!(matches!(m.err(), KeyError::WrongKey { .. }));
    }

    #[test]
    fn a_key_or_directory_others_can_reach_is_refused() {
        for bad in [0o644, 0o640, 0o604, 0o660, 0o606, 0o666, 0o700 | 0o010] {
            let m = machine(&format!("mode-{bad:o}"));
            m.first_start();
            set_mode(&m.key_path(), bad);
            assert!(matches!(m.err(), KeyError::KeyAccessible { .. }), "{bad:o}");
        }
        for ok in [0o600, 0o400] {
            let m = machine(&format!("mode-ok-{ok:o}"));
            m.first_start();
            set_mode(&m.key_path(), ok);
            assert!(prepare(&m.config, &m.log, &mut Failing).is_ok(), "{ok:o}");
        }
        let m = machine("dir-mode");
        m.first_start();
        set_mode(&m.config, 0o755);
        assert!(matches!(m.err(), KeyError::DirAccessible { .. }));
    }

    /// A user who is not the owner of anything the test made.
    fn someone_else() -> u32 {
        current_user().wrapping_add(1)
    }

    #[test]
    fn a_key_another_user_owns_is_refused_however_private_its_mode() {
        let m = machine("key-owner");
        m.first_start();
        assert_eq!(mode(&m.key_path()), 0o600);
        let me = current_user();
        let other = someone_else();
        // The same file, the same mode: only who is running differs.
        assert!(load_as(&m.key_path(), me).is_ok());
        let err = load_as(&m.key_path(), other).expect_err("refuse");
        assert!(
            matches!(&err, KeyError::KeyNotOwned { owner, user, .. } if (*owner, *user) == (me, other)),
            "{err}"
        );
        let said = err.to_string();
        assert!(said.contains(&format!("owned by user id {me}")), "{said}");
        assert!(
            said.contains(&format!("running as user id {other}")),
            "{said}"
        );
        assert!(said.contains("chown"), "it says what to do: {said}");
        assert!(!said.contains(&"5a".repeat(8)), "and does not echo the key");
        // Starting the engine reads the key through the same check; with the directory owned too, which
        // tests cannot arrange without root, that is the case in the real-chown test below.
        assert!(prepare_as(&m.config, &m.log, &mut Failing, me).is_ok());
    }

    #[test]
    fn a_config_directory_another_user_owns_is_refused_before_anything_is_read_or_made() {
        let m = machine("dir-owner");
        m.first_start();
        let other = someone_else();
        let err = prepare_as(&m.config, &m.log, &mut Failing, other).expect_err("refuse");
        assert!(
            matches!(&err, KeyError::DirNotOwned { owner, user, .. } if (*owner, *user) == (current_user(), other)),
            "{err}"
        );
        assert!(err.to_string().contains("chown"), "{err}");

        // On a first start too: no key is made in a directory the user does not own.
        let fresh = machine("dir-owner-fresh");
        let err = prepare_as(&fresh.config, &fresh.log, &mut Fixed(Some(SEED)), other)
            .expect_err("refuse");
        assert!(matches!(err, KeyError::DirNotOwned { .. }), "{err}");
        assert!(!exists(&fresh.key_path()) && !exists(&fresh.public_path()));
    }

    /// With the real thing: needs a user who may give a file away, so it runs where the tests run as root
    /// (a container) and says so where they do not. The two tests above cover the same checks anywhere.
    #[test]
    fn a_key_really_given_to_another_user_is_refused() {
        if !rustix::process::geteuid().is_root() {
            eprintln!("skipped: chown to another user needs root");
            return;
        }
        let m = machine("real-chown");
        m.first_start();
        let nobody = 65_534;
        std::os::unix::fs::chown(m.key_path(), Some(nobody), None).expect("chown the key");
        assert!(matches!(m.err(), KeyError::KeyNotOwned { owner, .. } if owner == nobody));
        std::os::unix::fs::chown(m.key_path(), Some(0), None).expect("give it back");
        std::os::unix::fs::chown(&m.config, Some(nobody), None).expect("chown the directory");
        assert!(matches!(m.err(), KeyError::DirNotOwned { owner, .. } if owner == nobody));
    }

    #[test]
    fn a_symlink_a_directory_or_a_wrong_file_is_not_a_key() {
        let m = machine("not-a-key");
        m.first_start();
        let real = m.config.join("real.key");
        fs::rename(m.key_path(), &real).expect("move");
        symlink(&real, m.key_path()).expect("symlink");
        assert!(matches!(m.err(), KeyError::NotRegular(_)));
        fs::remove_file(m.key_path()).expect("unlink");
        fs::create_dir(m.key_path()).expect("dir");
        assert!(matches!(m.err(), KeyError::NotRegular(_)));
        fs::remove_dir(m.key_path()).expect("rmdir");

        let secret = "5a".repeat(32);
        for contents in [
            format!("{secret}\n"),
            format!("hyalos-engine-key-v2:{secret}\n"),
            format!("hyalos-engine-key-v1:{}\n", &secret[..62]),
            format!("hyalos-engine-key-v1:{secret}\nextra\n"),
            format!("hyalos-engine-key-v1:{}zz\n", &secret[..62]),
            String::new(),
        ] {
            fs::write(m.key_path(), &contents).expect("write");
            set_mode(&m.key_path(), 0o600);
            let err = m.err();
            assert!(matches!(err, KeyError::Malformed(_)), "{contents:?}: {err}");
            assert!(
                !err.to_string().contains(&secret[..16]),
                "the error must not echo the file"
            );
        }
    }

    #[test]
    fn an_interrupted_run_leaves_a_temporary_file_that_is_refused_not_trusted() {
        let m = machine("stale-temp");
        fs::create_dir_all(&m.config).expect("dir");
        set_mode(&m.config, 0o700);
        fs::write(m.config.join("engine.key.tmp"), "half a key").expect("stale");
        let err = prepare(&m.config, &m.log, &mut Fixed(Some(SEED))).expect_err("refuse");
        assert!(matches!(err, KeyError::StaleTemporary(_)), "{err}");
        assert!(!exists(&m.key_path()));
        assert_eq!(
            fs::read(m.config.join("engine.key.tmp")).expect("untouched"),
            b"half a key"
        );
    }

    #[test]
    fn a_failed_entropy_source_creates_no_key() {
        let m = machine("no-entropy");
        let err = prepare(&m.config, &m.log, &mut Failing).expect_err("refuse");
        assert!(matches!(err, KeyError::Entropy(_)), "{err}");
        assert!(!exists(&m.key_path()) && !exists(&m.public_path()));
        assert!(!exists(&m.config.join("engine.key.tmp")));
    }

    #[test]
    fn a_crash_between_the_key_and_its_public_half_is_repaired_on_the_next_start() {
        let m = machine("crash");
        m.first_start();
        set_mode(&m.public_path(), 0o644);
        fs::remove_file(m.public_path()).expect("simulate the crash");
        let ready = prepare(&m.config, &m.log, &mut Failing).expect("repair");
        assert!(ready.public_rewritten && !ready.created);
    }

    #[test]
    fn the_real_entropy_source_gives_a_different_key_each_time() {
        let (a, b) = (machine("os-a"), machine("os-b"));
        let ka = prepare(&a.config, &a.log, &mut OsSeed).expect("a").public;
        let kb = prepare(&b.config, &b.log, &mut OsSeed).expect("b").public;
        assert_ne!(ka, kb);
        assert_ne!(ka, LogKey::from_seed([0; 32]).public());
    }

    #[test]
    fn restoring_on_another_machine_with_the_key_lets_the_engine_carry_on() {
        let old = machine("restore-old");
        old.first_start();
        append_events(&old.log, LogKey::from_seed(SEED), 4);
        let head = Log::open(&old.log, LogKey::from_seed(SEED))
            .expect("open")
            .head()
            .expect("head");
        let backup = old.config.join("backup.db");
        copy_verified(&old.log, &backup, &LogKey::from_seed(SEED).public(), head).expect("backup");

        // A machine with nothing: restore the log, put the key back, run init.
        let new = machine("restore-new");
        fs::create_dir_all(new.log.parent().expect("data dir")).expect("dir");
        copy_verified(&backup, &new.log, &LogKey::from_seed(SEED).public(), head).expect("restore");
        let err = new.err();
        assert!(
            matches!(err, KeyError::LostKey { .. }),
            "a restored log without its key is frozen: {err}"
        );

        make_dir(&new.config).expect("config dir");
        fs::copy(old.key_path(), new.key_path()).expect("carry the key across");
        set_mode(&new.key_path(), 0o600);
        let ready = prepare(&new.config, &new.log, &mut Failing).expect("init after restore");
        assert!(ready.public_rewritten && !ready.created);
        assert_eq!(ready.public, LogKey::from_seed(SEED).public());
        verify(&new.log, &ready.public, head).expect("the restored log verifies");

        let mut engine = Engine::open(&new.log, ready.key).expect("engine opens the restored log");
        let next = engine
            .submit(
                Timestamp::from_millis(99),
                Origin::Engine,
                Event::TaskCreated {
                    request: Digest::from_bytes([9; 32]),
                },
            )
            .expect("and appends to it");
        assert_eq!(next.seq, 5);
        verify(&new.log, &ready.public, Some(next)).expect("the extended log verifies");
    }

    #[test]
    fn the_key_never_appears_in_the_log_the_public_file_or_any_printed_form() {
        let m = machine("no-leak");
        let ready = m.first_start();
        append_events(&m.log, LogKey::from_seed(SEED), 5);

        let secret_hex = "5a".repeat(32);
        let secret_raw = SEED;
        let mut files = Vec::new();
        for name in ["events.db", "events.db-wal", "events.db-shm"] {
            if let Ok(bytes) = fs::read(m.log.with_file_name(name)) {
                files.push((name.to_string(), bytes));
            }
        }
        files.push(("engine.pub".into(), fs::read(m.public_path()).expect("pub")));
        for (name, bytes) in &files {
            assert!(!contains(bytes, &secret_raw), "{name} holds the raw key");
            assert!(
                !contains(bytes, secret_hex.as_bytes()),
                "{name} holds the key as hex"
            );
        }

        // Everything that can be printed about the key, a result holding it, or an error about it.
        let printed = [
            format!("{ready:?}"),
            format!("{:?}", ready.key),
            format!("{:?}", Ok::<_, KeyError>(&ready)),
            format!("{:#?}", ready),
        ];
        let keys = m.err_from_malformed(&secret_hex);
        for text in printed.iter().chain(std::iter::once(&keys)) {
            assert!(!text.contains(&secret_hex), "printed the key: {text}");
            assert!(
                !text.contains(&format!("{secret_raw:?}")),
                "printed the key bytes: {text}"
            );
        }

        // And the text a panic carries when someone unwraps a failure: `Result::expect` prints its
        // message followed by the error's Debug form, for every error this module can return.
        let errors = [
            KeyError::Malformed(m.key_path()),
            KeyError::KeyAccessible {
                path: m.key_path(),
                mode: 0o644,
            },
            KeyError::LostKey {
                key: m.key_path(),
                log: m.log.clone(),
                public: m.public_path(),
            },
            KeyError::Entropy("no entropy source".into()),
        ];
        for error in errors {
            let panic_text = format!("start: {error:?}");
            assert!(!panic_text.contains(&secret_hex), "{panic_text}");
        }
    }

    impl Machine {
        /// The message for a key file whose contents are wrong, built from a real secret.
        fn err_from_malformed(&self, secret_hex: &str) -> String {
            let path = self.config.join("other.key");
            fs::write(&path, format!("hyalos-engine-key-v9:{secret_hex}\n")).expect("write");
            set_mode(&path, 0o600);
            let err = load(&path).expect_err("malformed");
            format!("{err} / {err:?}")
        }
    }
}
