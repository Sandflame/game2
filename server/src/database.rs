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
use shared::classes::SecondaryChoice;
use shared::components::PlayerId;
use shared::progression::ClassProgress;

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
            "SELECT id, class, zone, x, y, z, yaw FROM characters WHERE name = ?1",
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
    Ok(Some(save))
}

enum Job {
    Load {
        player: PlayerId,
        name: String,
    },
    Save(Box<CharacterSave>),
    /// Reply once everything sent before this is done.
    Flush(Sender<()>),
}

/// A character finished loading (`None`: no save yet, make a new one).
pub struct Loaded {
    pub player: PlayerId,
    pub name: String,
    pub save: Option<CharacterSave>,
}

/// The save file, worked on by a background thread. Without this resource
/// (e.g. in tests) nothing is saved and every character starts fresh.
#[derive(Resource)]
pub struct Database {
    jobs: Sender<Job>,
    loaded: Mutex<Receiver<Loaded>>,
}

/// How long the game waits for saving to finish when it closes.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

impl Database {
    /// Open the save file and start the database thread.
    pub fn start(path: &Path) -> Result<Self, DatabaseError> {
        let mut conn = open(path)?;
        let (jobs, inbox) = channel::<Job>();
        let (outbox, loaded) = channel::<Loaded>();
        // The thread ends by itself when this `Database` is dropped (its job
        // queue closes).
        std::thread::Builder::new()
            .name("database".into())
            .spawn(move || {
                for job in inbox {
                    match job {
                        Job::Load { player, name } => {
                            let save = load_character(&conn, &name).unwrap_or_else(|error| {
                                error!("could not load character `{name}`: {error}");
                                None
                            });
                            if outbox.send(Loaded { player, name, save }).is_err() {
                                break;
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
        })
    }

    pub fn load(&self, player: PlayerId, name: &str) {
        let _ = self.jobs.send(Job::Load {
            player,
            name: name.to_owned(),
        });
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
        }
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
        db.load(PlayerId(1), "Ari");
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
