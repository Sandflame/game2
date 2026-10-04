//! Loading and validating RON data files from `assets/data/`.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

/// Environment variable that overrides where the `assets` folder is.
pub const ASSETS_DIR_ENV: &str = "LANTERNFLAME_ASSETS";

/// Something went wrong reading a data file. Every variant names the file
/// so the person editing it knows where to look.
#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("could not read data file {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse data file {path}: {source}")]
    Parse {
        path: PathBuf,
        source: Box<ron::error::SpannedError>,
    },
    #[error("invalid values in data file {path}:\n  - {}", problems.join("\n  - "))]
    Invalid {
        path: PathBuf,
        problems: Vec<String>,
    },
    #[error(
        "could not find the `assets` folder. Put it next to the program, run from the \
         project folder, or set the {ASSETS_DIR_ENV} environment variable"
    )]
    AssetsNotFound,
}

/// Data types implement this to check their values after loading.
/// Return one human-readable message per problem found.
pub trait Validate {
    fn validate(&self) -> Vec<String>;
}

/// Read and validate one RON file.
pub fn load_ron<T: DeserializeOwned + Validate>(path: &Path) -> Result<T, DataError> {
    let text = std::fs::read_to_string(path).map_err(|source| DataError::Io {
        path: path.to_owned(),
        source,
    })?;
    parse_ron(&text, path)
}

/// Parse and validate RON text. `path` is only used in error messages.
pub fn parse_ron<T: DeserializeOwned + Validate>(text: &str, path: &Path) -> Result<T, DataError> {
    let value: T = ron::from_str(text).map_err(|source| DataError::Parse {
        path: path.to_owned(),
        source: Box::new(source),
    })?;
    let problems = value.validate();
    if problems.is_empty() {
        Ok(value)
    } else {
        Err(DataError::Invalid {
            path: path.to_owned(),
            problems,
        })
    }
}

/// Find the `assets` folder. Checked in order:
/// 1. the `LANTERNFLAME_ASSETS` environment variable,
/// 2. next to the running program,
/// 3. the current working directory,
/// 4. the project folder this was compiled from (for `cargo run`).
pub fn find_assets_dir() -> Result<PathBuf, DataError> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os(ASSETS_DIR_ENV) {
        candidates.push(PathBuf::from(dir));
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        candidates.push(exe_dir.join("assets"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("assets"));
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets"));

    candidates
        .into_iter()
        .find(|dir| dir.join("data").is_dir())
        .map(|dir| dir.canonicalize().unwrap_or(dir))
        .ok_or(DataError::AssetsNotFound)
}

/// Collects validation problems with less boilerplate.
#[derive(Default)]
pub struct Problems(pub Vec<String>);

impl Problems {
    /// Record a problem unless `value` is a finite number greater than zero.
    pub fn positive(&mut self, name: &str, value: f32) {
        if !(value.is_finite() && value > 0.0) {
            self.0
                .push(format!("`{name}` must be greater than 0 (got {value})"));
        }
    }

    /// Record a problem unless `value` is a finite number of at least zero.
    pub fn non_negative(&mut self, name: &str, value: f32) {
        if !(value.is_finite() && value >= 0.0) {
            self.0
                .push(format!("`{name}` must be 0 or more (got {value})"));
        }
    }

    pub fn push(&mut self, message: impl Into<String>) {
        self.0.push(message.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Speed {
        value: f32,
    }

    impl Validate for Speed {
        fn validate(&self) -> Vec<String> {
            let mut p = Problems::default();
            p.positive("value", self.value);
            p.0
        }
    }

    #[test]
    fn parses_valid_ron() {
        let speed: Speed = parse_ron("(value: 2.5)", Path::new("speed.ron")).unwrap();
        assert_eq!(speed.value, 2.5);
    }

    #[test]
    fn parse_error_names_the_file() {
        let err = parse_ron::<Speed>("(value: oops)", Path::new("speed.ron")).unwrap_err();
        assert!(matches!(err, DataError::Parse { .. }));
        assert!(err.to_string().contains("speed.ron"));
    }

    #[test]
    fn validation_error_names_file_and_field() {
        let err = parse_ron::<Speed>("(value: -1.0)", Path::new("speed.ron")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("speed.ron"), "{msg}");
        assert!(msg.contains("`value`"), "{msg}");
    }

    #[test]
    fn finds_the_project_assets_folder() {
        let dir = find_assets_dir().unwrap();
        assert!(dir.join("data").is_dir());
    }
}
