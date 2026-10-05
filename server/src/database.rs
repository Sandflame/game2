//! Saving characters in one SQLite file (`world.db`).
//!
//! All database work happens on its own thread, so a slow disk never makes
//! the game stutter: the game sends jobs (load, save) and picks up loaded
//! characters later. Changes to the file's layout are numbered steps
//! ([`MIGRATIONS`]); an older file is backed up and then upgraded
//! automatically when the game starts.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use bevy::prelude::*;
use rusqlite::{Connection, OptionalExtension, params};
use shared::appearance::Appearance;
use shared::classes::SecondaryChoice;
use shared::components::PlayerId;
use shared::progression::ClassProgress;
use shared::protocol::CharacterSummary;

/// Each step upgrades the file by one version. Never change a step that
/// has been released: add a new one instead.
pub const MIGRATIONS: &[&str] = &[
    // 1: characters, their class levels, and their items.
    "CREATE TABLE characters (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE,
        class TEXT NOT NULL,
        zone TEXT NOT NULL,
        x REAL NOT NULL, y REAL NOT NULL, z REAL NOT NULL,
        yaw REAL NOT NULL,
        saved_at INTEGER NOT NULL
    );
    CREATE TABLE class_levels (
        character_id INTEGER NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
        class TEXT NOT NULL,
        level INTEGER NOT NULL,
        xp INTEGER NOT NULL,
        PRIMARY KEY (character_id, class)
    );
    CREATE TABLE items (
        id INTEGER PRIMARY KEY,
        character_id INTEGER NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
        item TEXT NOT NULL,
        worn TEXT
    );",
    // 2: each class's secondary class and its two borrowed abilities.
    "CREATE TABLE secondary_choices (
        character_id INTEGER NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
        main_class TEXT NOT NULL,
        secondary_class TEXT NOT NULL,
        first_ability TEXT,
        second_ability TEXT,
        PRIMARY KEY (character_id, main_class)
    );",
    // 3: quests being done (step, enemies counted) and finished.
    "CREATE TABLE quests (
        character_id INTEGER NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
        quest TEXT NOT NULL,
        step INTEGER NOT NULL,
        count INTEGER NOT NULL,
        done INTEGER NOT NULL,
        PRIMARY KEY (character_id, quest)
    );",
    // 4: each class's chosen specialization.
    "CREATE TABLE class_specs (
        character_id INTEGER NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
        class TEXT NOT NULL,
        spec TEXT NOT NULL,
        PRIMARY KEY (character_id, class)
    );",
    // 5: accounts (argon2-hashed passwords), which account owns each
    // character, and what each character looks like (RON text).
    // `saved_at` 0 marks a character created but never played.
    "CREATE TABLE accounts (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE COLLATE NOCASE,
        password TEXT NOT NULL,
        created_at INTEGER NOT NULL
    );
    ALTER TABLE characters ADD COLUMN account_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE;
    ALTER TABLE characters ADD COLUMN appearance TEXT;",
];

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("could not open the save file {path}: {source}")]
    Open {
        path: PathBuf,
        source: rusqlite::Error,
    },
    #[error("could not back up {path} before upgrading it: {source}")]
    Backup {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not upgrade the save file {path} to version {version}: {source}")]
    Upgrade {
        path: PathBuf,
        version: usize,
        source: rusqlite::Error,
    },
    #[error(
        "the save file {path} is from a newer version of the game (version {found}, this game knows up to {known})"
    )]
    TooNew {
        path: PathBuf,
        found: usize,
        known: usize,
    },
}

/// One character as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct CharacterSave {
    pub name: String,
    pub class: String,
    pub zone: String,
    pub position: Vec3,
    pub yaw: f32,
    pub levels: Vec<(String, ClassProgress)>,
    pub items: Vec<SavedItem>,
    /// (main class, its secondary choice).
    pub secondaries: Vec<(String, SecondaryChoice)>,
    pub quests: Vec<SavedQuest>,
    /// (class, its chosen specialization).
    pub specs: Vec<(String, String)>,
    /// Chosen when the character was created (older saves have none).
    pub appearance: Option<Appearance>,
    /// Has it ever been in the world? A character only just created starts
    /// fresh (starting gear, the starting zone).
    pub played: bool,
}

/// A quest being done (its step and enemies counted) or finished.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedQuest {
    pub quest: String,
    pub step: u32,
    pub count: u32,
    pub done: bool,
}

/// An owned item and where it is worn: `None` in the bag, `"head"` etc. for
/// armour, `"weapon:<class>"` for a class's weapon.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedItem {
    pub item: String,
    pub worn: Option<String>,
}

fn version(conn: &Connection) -> rusqlite::Result<usize> {
    conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map(|v| v.max(0) as usize)
}

/// Bring a database up to the newest layout. Returns the version it was at.
pub fn migrate(conn: &mut Connection, path: &Path) -> Result<usize, DatabaseError> {
    let found = version(conn).map_err(|source| DatabaseError::Open {
        path: path.to_owned(),
        source,
    })?;
    if found > MIGRATIONS.len() {
        return Err(DatabaseError::TooNew {
            path: path.to_owned(),
            found,
            known: MIGRATIONS.len(),
        });
    }
    for (index, step) in MIGRATIONS.iter().enumerate().skip(found) {
        let version = index + 1;
        let upgrade = |source| DatabaseError::Upgrade {
            path: path.to_owned(),
            version,
            source,
        };
        let tx = conn.transaction().map_err(upgrade)?;
        tx.execute_batch(step).map_err(upgrade)?;
        tx.pragma_update(None, "user_version", version as i64)
            .map_err(upgrade)?;
        tx.commit().map_err(upgrade)?;
    }
    Ok(found)
}

/// Open (or create) the save file, backing it up first if it needs upgrading.
pub fn open(path: &Path) -> Result<Connection, DatabaseError> {
    let open_error = |source| DatabaseError::Open {
        path: path.to_owned(),
        source,
    };
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).map_err(|source| DatabaseError::Backup {
            path: dir.to_owned(),
            source,
        })?;
    }
    let existed = path.exists();
    let mut conn = Connection::open(path).map_err(open_error)?;
    conn.pragma_update(None, "foreign_keys", true)
        .map_err(open_error)?;
    let found = version(&conn).map_err(open_error)?;
    if existed && found > 0 && found < MIGRATIONS.len() {
        let backup = path.with_extension(format!("backup-v{found}.db"));
        std::fs::copy(path, &backup).map_err(|source| DatabaseError::Backup {
            path: backup.clone(),
            source,
        })?;
        info!("backed up {} to {}", path.display(), backup.display());
    }
    migrate(&mut conn, path)?;
    Ok(conn)
}

/// Write a character (replacing what was saved before).
pub fn save_character(conn: &mut Connection, save: &CharacterSave) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    tx.execute(
        "INSERT INTO characters (name, class, zone, x, y, z, yaw, saved_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(name) DO UPDATE SET class = ?2, zone = ?3, x = ?4, y = ?5,
             z = ?6, yaw = ?7, saved_at = ?8",
        params![
            save.name,
            save.class,
            save.zone,
            save.position.x,
            save.position.y,
            save.position.z,
            save.yaw,
            now
        ],
    )?;
    let id: i64 = tx.query_row(
        "SELECT id FROM characters WHERE name = ?1",
        params![save.name],
        |row| row.get(0),
    )?;
    tx.execute(
        "DELETE FROM class_levels WHERE character_id = ?1",
        params![id],
    )?;
    tx.execute("DELETE FROM items WHERE character_id = ?1", params![id])?;
    tx.execute(
        "DELETE FROM secondary_choices WHERE character_id = ?1",
        params![id],
    )?;
    tx.execute(
        "DELETE FROM class_specs WHERE character_id = ?1",
        params![id],
    )?;
    for (class, spec) in &save.specs {
        tx.execute(
            "INSERT INTO class_specs (character_id, class, spec) VALUES (?1, ?2, ?3)",
            params![id, class, spec],
        )?;
    }
    tx.execute("DELETE FROM quests WHERE character_id = ?1", params![id])?;
    for quest in &save.quests {
        tx.execute(
            "INSERT INTO quests (character_id, quest, step, count, done)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, quest.quest, quest.step, quest.count, quest.done],
        )?;
    }
    for (main, choice) in &save.secondaries {
        tx.execute(
            "INSERT INTO secondary_choices
             (character_id, main_class, secondary_class, first_ability, second_ability)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                main,
                choice.class,
                choice.abilities[0],
                choice.abilities[1]
            ],
        )?;
    }
    for (class, progress) in &save.levels {
        tx.execute(
            "INSERT INTO class_levels (character_id, class, level, xp) VALUES (?1, ?2, ?3, ?4)",
            params![id, class, progress.level, progress.xp],
        )?;
    }
    for item in &save.items {
        tx.execute(
            "INSERT INTO items (character_id, item, worn) VALUES (?1, ?2, ?3)",
            params![id, item.item, item.worn],
        )?;
    }
    tx.commit()
}

/// Read a character by name, if one was saved.
pub fn load_character(conn: &Connection, name: &str) -> rusqlite::Result<Option<CharacterSave>> {
    let Some((id, mut save)) = conn
        .query_row(
            "SELECT id, class, zone, x, y, z, yaw, saved_at, appearance
             FROM characters WHERE name = ?1",
            params![name],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    CharacterSave {
                        name: name.to_owned(),
                        class: row.get(1)?,
                        zone: row.get(2)?,
                        position: Vec3::new(row.get(3)?, row.get(4)?, row.get(5)?),
                        yaw: row.get(6)?,
                        levels: Vec::new(),
                        items: Vec::new(),
                        secondaries: Vec::new(),
                        quests: Vec::new(),
                        specs: Vec::new(),
                        played: row.get::<_, i64>(7)? > 0,
                        appearance: read_appearance(row.get(8)?),
                    },
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let mut levels = conn.prepare(
        "SELECT class, level, xp FROM class_levels WHERE character_id = ?1 ORDER BY class",
    )?;
    save.levels = levels
        .query_map(params![id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                ClassProgress {
                    level: row.get(1)?,
                    xp: row.get(2)?,
                },
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut items =
        conn.prepare("SELECT item, worn FROM items WHERE character_id = ?1 ORDER BY id")?;
    save.items = items
        .query_map(params![id], |row| {
            Ok(SavedItem {
                item: row.get(0)?,
                worn: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut secondaries = conn.prepare(
        "SELECT main_class, secondary_class, first_ability, second_ability
         FROM secondary_choices WHERE character_id = ?1 ORDER BY main_class",
    )?;
    save.secondaries = secondaries
        .query_map(params![id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                SecondaryChoice {
                    class: row.get(1)?,
                    abilities: [row.get(2)?, row.get(3)?],
                },
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut quests = conn.prepare(
        "SELECT quest, step, count, done FROM quests WHERE character_id = ?1 ORDER BY quest",
    )?;
    save.quests = quests
        .query_map(params![id], |row| {
            Ok(SavedQuest {
                quest: row.get(0)?,
                step: row.get(1)?,
                count: row.get(2)?,
                done: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut specs =
        conn.prepare("SELECT class, spec FROM class_specs WHERE character_id = ?1 ORDER BY class")?;
    save.specs = specs
        .query_map(params![id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(save))
}

fn read_appearance(text: Option<String>) -> Option<Appearance> {
    text.and_then(|t| ron::from_str(&t).ok())
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Why an account or character request was refused (shown to the player).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AccountError {
    #[error("That account name is taken.")]
    NameTaken,
    #[error("Wrong account name or password.")]
    WrongLogin,
    #[error("That character name is taken.")]
    CharacterTaken,
    #[error("You already have {0} characters, the most an account can have.")]
    TooMany(usize),
    #[error("You have no character called {0}.")]
    NoSuchCharacter(String),
    #[error("Something went wrong with the save file: {0}")]
    Database(String),
}

impl From<rusqlite::Error> for AccountError {
    fn from(error: rusqlite::Error) -> Self {
        AccountError::Database(error.to_string())
    }
}

/// Make an account. The first account made takes over any characters from
/// before accounts existed. Returns its id.
pub fn register(conn: &mut Connection, name: &str, password: &str) -> Result<i64, AccountError> {
    let taken: bool = conn
        .query_row(
            "SELECT 1 FROM accounts WHERE name = ?1",
            params![name],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if taken {
        return Err(AccountError::NameTaken);
    }
    let hash = hash_password(password)?;
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO accounts (name, password, created_at) VALUES (?1, ?2, ?3)",
        params![name, hash, now_seconds()],
    )?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "UPDATE characters SET account_id = ?1 WHERE account_id IS NULL",
        params![id],
    )?;
    tx.commit()?;
    Ok(id)
}

fn hash_password(password: &str) -> Result<String, AccountError> {
    use argon2::password_hash::PasswordHasher;
    argon2::Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|e| AccountError::Database(e.to_string()))
}

/// Check a login. Returns the account's id and its name as registered.
pub fn login(conn: &Connection, name: &str, password: &str) -> Result<(i64, String), AccountError> {
    use argon2::password_hash::PasswordVerifier;
    use argon2::password_hash::phc::PasswordHash;
    let Some((id, registered, stored)) = conn
        .query_row(
            "SELECT id, name, password FROM accounts WHERE name = ?1",
            params![name],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
    else {
        return Err(AccountError::WrongLogin);
    };
    let hash = PasswordHash::new(&stored).map_err(|_| AccountError::WrongLogin)?;
    argon2::Argon2::default()
        .verify_password(password.as_bytes(), &hash)
        .map_err(|_| AccountError::WrongLogin)?;
    Ok((id, registered))
}

/// An account's characters, oldest first.
pub fn list_characters(
    conn: &Connection,
    account: i64,
) -> Result<Vec<CharacterSummary>, AccountError> {
    let mut query = conn.prepare(
        "SELECT c.name, c.class, c.zone, c.appearance,
                COALESCE((SELECT level FROM class_levels l
                          WHERE l.character_id = c.id AND l.class = c.class), 1)
         FROM characters c WHERE c.account_id = ?1 ORDER BY c.id",
    )?;
    let list = query
        .query_map(params![account], |row| {
            Ok(CharacterSummary {
                name: row.get(0)?,
                class: row.get(1)?,
                zone: row.get(2)?,
                appearance: read_appearance(row.get(3)?),
                level: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(list)
}

/// A brand-new character for an account (not played yet).
pub struct NewCharacter<'a> {
    pub name: &'a str,
    pub class: &'a str,
    pub appearance: &'a Appearance,
    pub zone: &'a str,
    pub position: Vec3,
    pub yaw: f32,
}

pub fn create_character(
    conn: &mut Connection,
    account: i64,
    new: &NewCharacter,
    max_characters: usize,
) -> Result<(), AccountError> {
    let tx = conn.transaction()?;
    let count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM characters WHERE account_id = ?1",
        params![account],
        |row| row.get(0),
    )?;
    if count as usize >= max_characters {
        return Err(AccountError::TooMany(max_characters));
    }
    let taken = tx
        .query_row(
            "SELECT 1 FROM characters WHERE name = ?1 COLLATE NOCASE",
            params![new.name],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if taken {
        return Err(AccountError::CharacterTaken);
    }
    let appearance =
        ron::to_string(new.appearance).map_err(|e| AccountError::Database(e.to_string()))?;
    tx.execute(
        "INSERT INTO characters (name, class, zone, x, y, z, yaw, saved_at, account_id, appearance)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9)",
        params![
            new.name,
            new.class,
            new.zone,
            new.position.x,
            new.position.y,
            new.position.z,
            new.yaw,
            account,
            appearance
        ],
    )?;
    tx.commit()?;
    Ok(())
}

/// Delete one of an account's characters (and everything it owns).
pub fn delete_character(conn: &Connection, account: i64, name: &str) -> Result<(), AccountError> {
    let deleted = conn.execute(
        "DELETE FROM characters WHERE account_id = ?1 AND name = ?2",
        params![account, name],
    )?;
    if deleted == 0 {
        return Err(AccountError::NoSuchCharacter(name.to_owned()));
    }
    Ok(())
}

/// Does this account own a character with this name?
pub fn owns(conn: &Connection, account: i64, name: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT 1 FROM characters WHERE account_id = ?1 AND name = ?2",
        params![account, name],
        |_| Ok(()),
    )
    .optional()
    .map(|found| found.is_some())
}

/// Account and character work the database thread does for a player.
pub enum AccountJob {
    Register {
        name: String,
        password: String,
    },
    Login {
        name: String,
        password: String,
    },
    List {
        account: i64,
    },
    Create {
        account: i64,
        new: NewCharacterOwned,
        max: usize,
    },
    Delete {
        account: i64,
        name: String,
    },
}

/// [`NewCharacter`], owned, to send to the database thread.
pub struct NewCharacterOwned {
    pub name: String,
    pub class: String,
    pub appearance: Appearance,
    pub zone: String,
    pub position: Vec3,
    pub yaw: f32,
}

/// The answer to an [`AccountJob`].
#[derive(Debug, Clone, PartialEq)]
pub enum AccountReply {
    LoggedIn { account: i64, name: String },
    Characters(Vec<CharacterSummary>),
    Failed(String),
}

fn account_job(conn: &mut Connection, job: AccountJob) -> Vec<AccountReply> {
    let characters = |conn: &Connection, account| match list_characters(conn, account) {
        Ok(list) => AccountReply::Characters(list),
        Err(e) => AccountReply::Failed(e.to_string()),
    };
    let result = match job {
        AccountJob::Register { name, password } => {
            register(conn, &name, &password).map(|account| {
                vec![
                    AccountReply::LoggedIn { account, name },
                    characters(conn, account),
                ]
            })
        }
        AccountJob::Login { name, password } => {
            login(conn, &name, &password).map(|(account, name)| {
                vec![
                    AccountReply::LoggedIn { account, name },
                    characters(conn, account),
                ]
            })
        }
        AccountJob::List { account } => Ok(vec![characters(conn, account)]),
        AccountJob::Create { account, new, max } => {
            let borrowed = NewCharacter {
                name: &new.name,
                class: &new.class,
                appearance: &new.appearance,
                zone: &new.zone,
                position: new.position,
                yaw: new.yaw,
            };
            create_character(conn, account, &borrowed, max)
                .map(|()| vec![characters(conn, account)])
        }
        AccountJob::Delete { account, name } => {
            delete_character(conn, account, &name).map(|()| vec![characters(conn, account)])
        }
    };
    result.unwrap_or_else(|e| vec![AccountReply::Failed(e.to_string())])
}

enum Job {
    Load {
        player: PlayerId,
        name: String,
        /// Only load it if this account owns it.
        account: Option<i64>,
    },
    Account(PlayerId, AccountJob),
    Save(Box<CharacterSave>),
    /// Reply once everything sent before this is done.
    Flush(Sender<()>),
}

/// A character finished loading (`None`: no save yet, make a new one).
pub struct Loaded {
    pub player: PlayerId,
    pub name: String,
    pub save: Option<CharacterSave>,
    /// Why it can't be played (it isn't this account's).
    pub refused: Option<String>,
}

/// The save file, worked on by a background thread. Without this resource
/// (e.g. in tests) nothing is saved and every character starts fresh.
#[derive(Resource)]
pub struct Database {
    jobs: Sender<Job>,
    loaded: Mutex<Receiver<Loaded>>,
    replies: Mutex<Receiver<(PlayerId, AccountReply)>>,
}

/// How long the game waits for saving to finish when it closes.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

impl Database {
    /// Open the save file and start the database thread.
    pub fn start(path: &Path) -> Result<Self, DatabaseError> {
        let mut conn = open(path)?;
        let (jobs, inbox) = channel::<Job>();
        let (outbox, loaded) = channel::<Loaded>();
        let (reply_box, replies) = channel::<(PlayerId, AccountReply)>();
        // The thread ends by itself when this `Database` is dropped (its job
        // queue closes).
        std::thread::Builder::new()
            .name("database".into())
            .spawn(move || {
                for job in inbox {
                    match job {
                        Job::Load {
                            player,
                            name,
                            account,
                        } => {
                            let refused = match account.map(|a| owns(&conn, a, &name)) {
                                Some(Ok(false)) => {
                                    Some(AccountError::NoSuchCharacter(name.clone()).to_string())
                                }
                                Some(Err(e)) => Some(AccountError::from(e).to_string()),
                                _ => None,
                            };
                            let save = if refused.is_some() {
                                None
                            } else {
                                load_character(&conn, &name).unwrap_or_else(|error| {
                                    error!("could not load character `{name}`: {error}");
                                    None
                                })
                            };
                            let loaded = Loaded {
                                player,
                                name,
                                save,
                                refused,
                            };
                            if outbox.send(loaded).is_err() {
                                break;
                            }
                        }
                        Job::Account(player, job) => {
                            for reply in account_job(&mut conn, job) {
                                if reply_box.send((player, reply)).is_err() {
                                    return;
                                }
                            }
                        }
                        Job::Save(save) => {
                            if let Err(error) = save_character(&mut conn, &save) {
                                error!("could not save character `{}`: {error}", save.name);
                            }
                        }
                        Job::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            })
            .map_err(|source| DatabaseError::Backup {
                path: path.to_owned(),
                source,
            })?;
        Ok(Self {
            jobs,
            loaded: Mutex::new(loaded),
            replies: Mutex::new(replies),
        })
    }

    /// Load a character to play. With an account, only one it owns.
    pub fn load(&self, player: PlayerId, name: &str, account: Option<i64>) {
        let _ = self.jobs.send(Job::Load {
            player,
            name: name.to_owned(),
            account,
        });
    }

    pub fn account(&self, player: PlayerId, job: AccountJob) {
        let _ = self.jobs.send(Job::Account(player, job));
    }

    /// Answers to account jobs that have finished.
    pub fn take_replies(&self) -> Vec<(PlayerId, AccountReply)> {
        self.replies
            .lock()
            .map(|inbox| inbox.try_iter().collect())
            .unwrap_or_default()
    }

    pub fn save(&self, save: CharacterSave) {
        let _ = self.jobs.send(Job::Save(Box::new(save)));
    }

    /// Characters that have finished loading.
    pub fn take_loaded(&self) -> Vec<Loaded> {
        self.loaded
            .lock()
            .map(|inbox| inbox.try_iter().collect())
            .unwrap_or_default()
    }

    /// Wait (briefly) until every save sent so far is written.
    pub fn flush(&self) {
        let (done, wait) = channel();
        if self.jobs.send(Job::Flush(done)).is_ok() && wait.recv_timeout(FLUSH_TIMEOUT).is_err() {
            warn!("saving took too long; some progress may not be saved");
        }
    }
}

/// Where the world is saved: `LANTERNFLAME_DB` if set, otherwise
/// - Windows: `%APPDATA%\Lanternflame\world.db`
/// - Linux: `~/.local/share/lanternflame/world.db`
///
/// The game on this computer and the `server` program use the same file,
/// so characters made in one can be played in the other (just not both at
/// once).
pub fn save_file_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Some(path) = std::env::var_os("LANTERNFLAME_DB") {
        return Some(PathBuf::from(path));
    }
    if cfg!(windows) {
        let base = std::env::var_os("APPDATA")?;
        return Some(PathBuf::from(base).join("Lanternflame").join("world.db"));
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })?;
    Some(base.join("lanternflame").join("world.db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> CharacterSave {
        CharacterSave {
            name: "Ari".into(),
            class: "priest".into(),
            zone: "sandbox".into(),
            position: Vec3::new(1.0, 0.5, -3.0),
            yaw: 0.25,
            levels: vec![
                ("blademaster".into(), ClassProgress { level: 4, xp: 120 }),
                ("priest".into(), ClassProgress { level: 7, xp: 5 }),
            ],
            items: vec![
                SavedItem {
                    item: "bark_helm".into(),
                    worn: Some("head".into()),
                },
                SavedItem {
                    item: "rootcleaver".into(),
                    worn: Some("weapon:blademaster".into()),
                },
                SavedItem {
                    item: "mossy_boots".into(),
                    worn: None,
                },
            ],
            secondaries: vec![(
                "priest".into(),
                SecondaryChoice {
                    class: "blademaster".into(),
                    abilities: [Some("bm_battle_focus".into()), None],
                },
            )],
            quests: vec![
                SavedQuest {
                    quest: "down_the_root".into(),
                    step: 1,
                    count: 0,
                    done: false,
                },
                SavedQuest {
                    quest: "lamplighters_errand".into(),
                    step: 1,
                    count: 0,
                    done: true,
                },
            ],
            specs: vec![("priest".into(), "judge".into())],
            appearance: None,
            played: true,
        }
    }

    fn look() -> Appearance {
        Appearance {
            race: "elf".into(),
            face: 1,
            skin: 2,
            hair: 0,
            feature: 1,
            feature_color: 0,
            height: 0.75,
        }
    }

    fn new_character<'a>(name: &'a str, appearance: &'a Appearance) -> NewCharacter<'a> {
        NewCharacter {
            name,
            class: "priest",
            appearance,
            zone: "hub",
            position: Vec3::new(1.0, 0.0, 2.0),
            yaw: 0.5,
        }
    }

    #[test]
    fn accounts_register_and_log_in() {
        let mut conn = memory();
        let id = register(&mut conn, "sandflame", "lantern").unwrap();
        assert_eq!(
            register(&mut conn, "SandFlame", "other"),
            Err(AccountError::NameTaken)
        );
        assert_eq!(
            login(&conn, "sandflame", "lantern").unwrap(),
            (id, "sandflame".into())
        );
        // Names ignore case; passwords don't.
        assert_eq!(login(&conn, "SANDFLAME", "lantern").unwrap().0, id);
        assert_eq!(
            login(&conn, "sandflame", "Lantern"),
            Err(AccountError::WrongLogin)
        );
        assert_eq!(
            login(&conn, "nobody", "lantern"),
            Err(AccountError::WrongLogin)
        );
        // The password is not stored as it is.
        let stored: String = conn
            .query_row("SELECT password FROM accounts", [], |r| r.get(0))
            .unwrap();
        assert!(!stored.contains("lantern") && stored.starts_with("$argon2"));
    }

    #[test]
    fn the_first_account_takes_over_older_characters() {
        let mut conn = memory();
        save_character(&mut conn, &example()).unwrap();
        let first = register(&mut conn, "first", "pass").unwrap();
        let second = register(&mut conn, "second", "pass").unwrap();
        let list = list_characters(&conn, first).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Ari");
        assert_eq!(list[0].level, 7, "the current class's level");
        assert!(list_characters(&conn, second).unwrap().is_empty());
    }

    #[test]
    fn characters_are_made_listed_and_deleted() {
        let mut conn = memory();
        let account = register(&mut conn, "owner", "pass").unwrap();
        let other = register(&mut conn, "other", "pass").unwrap();
        let elf = look();
        create_character(&mut conn, account, &new_character("Ari", &elf), 2).unwrap();
        assert_eq!(
            create_character(&mut conn, other, &new_character("ari", &elf), 2),
            Err(AccountError::CharacterTaken),
            "names are unique, ignoring case"
        );
        create_character(&mut conn, account, &new_character("Bryn", &elf), 2).unwrap();
        assert_eq!(
            create_character(&mut conn, account, &new_character("Cass", &elf), 2),
            Err(AccountError::TooMany(2))
        );
        let list = list_characters(&conn, account).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].appearance, Some(elf.clone()));
        assert_eq!(list[0].level, 1);
        // A new character hasn't been played yet; saving it marks it played
        // and keeps its look.
        let ari = load_character(&conn, "Ari").unwrap().unwrap();
        assert!(!ari.played);
        assert_eq!(ari.appearance, Some(elf.clone()));
        assert_eq!(ari.class, "priest");
        save_character(
            &mut conn,
            &CharacterSave {
                name: "Ari".into(),
                ..example()
            },
        )
        .unwrap();
        let ari = load_character(&conn, "Ari").unwrap().unwrap();
        assert!(ari.played);
        assert_eq!(ari.appearance, Some(elf));
        // Only the owner can delete (or play) it.
        assert!(owns(&conn, account, "Ari").unwrap());
        assert!(!owns(&conn, other, "Ari").unwrap());
        assert!(delete_character(&conn, other, "Ari").is_err());
        delete_character(&conn, account, "Ari").unwrap();
        assert_eq!(list_characters(&conn, account).unwrap().len(), 1);
        assert_eq!(load_character(&conn, "Ari").unwrap(), None);
    }

    fn memory() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, Path::new(":memory:")).unwrap();
        conn
    }

    #[test]
    fn characters_round_trip() {
        let mut conn = memory();
        assert_eq!(load_character(&conn, "Ari").unwrap(), None);
        save_character(&mut conn, &example()).unwrap();
        assert_eq!(load_character(&conn, "Ari").unwrap(), Some(example()));
    }

    #[test]
    fn saving_again_replaces_the_old_save() {
        let mut conn = memory();
        save_character(&mut conn, &example()).unwrap();
        let mut changed = example();
        changed.class = "blademaster".into();
        changed.items.truncate(1);
        changed.levels[0].1.level = 5;
        changed.secondaries.clear();
        save_character(&mut conn, &changed).unwrap();
        assert_eq!(load_character(&conn, "Ari").unwrap(), Some(changed));
    }

    #[test]
    fn migrations_run_once_and_set_the_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        let path = Path::new(":memory:");
        assert_eq!(migrate(&mut conn, path).unwrap(), 0);
        assert_eq!(version(&conn).unwrap(), MIGRATIONS.len());
        // Running again changes nothing.
        assert_eq!(migrate(&mut conn, path).unwrap(), MIGRATIONS.len());
    }

    #[test]
    fn old_files_are_backed_up_and_upgraded() {
        let dir = std::env::temp_dir().join(format!(
            "lanternflame-upgrade-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("world.db");
        {
            // A save file from version 1 (before secondary classes).
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO characters (name, class, zone, x, y, z, yaw, saved_at)
                 VALUES ('Ari', 'priest', 'sandbox', 0, 0, 0, 0, 0)",
                [],
            )
            .unwrap();
        }
        let conn = open(&path).unwrap();
        assert_eq!(version(&conn).unwrap(), MIGRATIONS.len());
        assert!(dir.join("world.backup-v1.db").exists(), "backed up first");
        let ari = load_character(&conn, "Ari").unwrap().unwrap();
        assert_eq!(ari.class, "priest");
        assert!(ari.secondaries.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn newer_files_are_refused() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        assert!(matches!(
            migrate(&mut conn, Path::new(":memory:")),
            Err(DatabaseError::TooNew { found: 99, .. })
        ));
    }

    #[test]
    fn the_background_thread_saves_and_loads() {
        let dir = std::env::temp_dir().join(format!(
            "lanternflame-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("world.db");
        let db = Database::start(&path).unwrap();
        db.save(example());
        db.load(PlayerId(1), "Ari", None);
        db.flush();
        let loaded = db.take_loaded();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].save, Some(example()));
        drop(db);
        // A second start opens the same file and finds the character.
        let again = open(&path).unwrap();
        assert!(load_character(&again, "Ari").unwrap().is_some());
        let _ = std::fs::remove_dir_all(dir);
    }
}
