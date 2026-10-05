//! What a character looks like: race, face and hair, colours, the race's
//! own feature (ears, horns, tails) and height. Races and their choices
//! are data (`assets/data/races.ron`); races are cosmetic only. Also the
//! rules for account and character names (`assets/data/accounts.ron`).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::data::{Problems, Validate};

/// A named choice (a face, a horn style…). The id names a look in the
/// client's `models.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub id: String,
    pub name: String,
}

/// A named colour (red, green, blue; 0–1).
#[derive(Debug, Clone, Deserialize)]
pub struct Swatch {
    pub name: String,
    pub color: (f32, f32, f32),
}

#[derive(Debug, Clone, Deserialize)]
pub struct RaceDef {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Faces with their hair (each a head in `models.ron`).
    pub faces: Vec<Choice>,
    pub skins: Vec<Swatch>,
    pub hair: Vec<Swatch>,
    /// The race's own feature, in several styles (empty for humans).
    #[serde(default)]
    pub features: Vec<Choice>,
    /// Colours for the feature (horns, scales…). Ears and fur follow the
    /// skin or hair instead and leave this empty.
    #[serde(default)]
    pub feature_colors: Vec<Swatch>,
    /// Shortest and tallest, as a multiple of the normal height.
    pub height: (f32, f32),
}

/// Every race, in the order the creation screen shows them.
#[derive(Debug, Clone, Deserialize)]
pub struct Races(pub Vec<RaceDef>);

impl Races {
    pub fn get(&self, id: &str) -> Option<&RaceDef> {
        self.0.iter().find(|race| race.id == id)
    }
}

impl Validate for Races {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        if self.0.is_empty() {
            p.push("there must be at least one race");
        }
        for (i, race) in self.0.iter().enumerate() {
            if self.0[..i].iter().any(|other| other.id == race.id) {
                p.push(format!("race id `{}` is used twice", race.id));
            }
            for (list, count) in [
                ("faces", race.faces.len()),
                ("skins", race.skins.len()),
                ("hair", race.hair.len()),
            ] {
                if count == 0 {
                    p.push(format!("{}.{list}: needs at least one choice", race.id));
                }
            }
            let (low, high) = race.height;
            p.positive(&format!("{}.height", race.id), low);
            if high < low {
                p.push(format!(
                    "{}.height: the tallest is below the shortest",
                    race.id
                ));
            }
        }
        p.0
    }
}

/// A character's look. Choices are positions in the race's lists.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    pub race: String,
    pub face: usize,
    pub skin: usize,
    pub hair: usize,
    #[serde(default)]
    pub feature: usize,
    #[serde(default)]
    pub feature_color: usize,
    /// 0 = the race's shortest, 1 = its tallest.
    pub height: f32,
}

impl Appearance {
    /// The first choice of everything for the first race.
    pub fn first(races: &Races) -> Self {
        Self {
            race: races.0.first().map(|r| r.id.clone()).unwrap_or_default(),
            face: 0,
            skin: 0,
            hair: 0,
            feature: 0,
            feature_color: 0,
            height: 0.5,
        }
    }

    /// Is every choice one the race offers?
    pub fn check(&self, races: &Races) -> Result<(), String> {
        let race = races
            .get(&self.race)
            .ok_or_else(|| format!("there is no race called `{}`", self.race))?;
        let within = |what: &str, index: usize, count: usize| {
            if index < count.max(1) {
                Ok(())
            } else {
                Err(format!("{} has no {what} number {}", race.name, index + 1))
            }
        };
        within("face", self.face, race.faces.len())?;
        within("skin colour", self.skin, race.skins.len())?;
        within("hair colour", self.hair, race.hair.len())?;
        within("feature", self.feature, race.features.len())?;
        within(
            "feature colour",
            self.feature_color,
            race.feature_colors.len(),
        )?;
        if !(0.0..=1.0).contains(&self.height) {
            return Err("height must be between 0 and 1".into());
        }
        Ok(())
    }

    /// Height as a multiple of the normal height.
    pub fn height_scale(&self, races: &Races) -> f32 {
        races.get(&self.race).map_or(1.0, |race| {
            let (low, high) = race.height;
            low + (high - low) * self.height.clamp(0.0, 1.0)
        })
    }
}

/// Rules for accounts and characters (`assets/data/accounts.ron`).
#[derive(Debug, Clone, Deserialize)]
pub struct AccountRules {
    pub max_characters: usize,
    /// Shortest and longest character name.
    pub character_name: (usize, usize),
    /// Shortest and longest account name.
    pub account_name: (usize, usize),
    pub password_min: usize,
}

impl Validate for AccountRules {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        if self.max_characters == 0 {
            p.push("`max_characters` must be at least 1");
        }
        for (name, (low, high)) in [
            ("character_name", self.character_name),
            ("account_name", self.account_name),
        ] {
            if low == 0 || high < low {
                p.push(format!("`{name}`: needs 0 < shortest <= longest"));
            }
        }
        p.0
    }
}

impl AccountRules {
    /// A character name, tidied (spaces trimmed and squeezed, first letters
    /// capitalised), or why it can't be used. Letters, spaces, `'` and `-`.
    pub fn character_name(&self, name: &str) -> Result<String, String> {
        let words: Vec<String> = name
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(chars).collect()
                })
            })
            .collect();
        let tidy = words.join(" ");
        let (low, high) = self.character_name;
        let length = tidy.chars().count();
        if length < low || length > high {
            return Err(format!("A name needs {low} to {high} letters."));
        }
        if !tidy
            .chars()
            .all(|c| c.is_alphabetic() || c == ' ' || c == '\'' || c == '-')
        {
            return Err("Names can only use letters, spaces, ' and -.".into());
        }
        if !tidy.chars().next().is_some_and(char::is_alphabetic) {
            return Err("A name must start with a letter.".into());
        }
        Ok(tidy)
    }

    /// An account name (letters, digits and `_`), or why it can't be used.
    pub fn account_name(&self, name: &str) -> Result<String, String> {
        let name = name.trim();
        let (low, high) = self.account_name;
        let length = name.chars().count();
        if length < low || length > high {
            return Err(format!("An account name needs {low} to {high} characters."));
        }
        if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err("Account names can only use letters, digits and _.".into());
        }
        Ok(name.to_owned())
    }

    pub fn password(&self, password: &str) -> Result<(), String> {
        if password.chars().count() < self.password_min {
            return Err(format!(
                "A password needs at least {} characters.",
                self.password_min
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn races() -> Races {
        ron::from_str(
            r#"([
                (id: "human", name: "Human", description: "", faces: [(id: "a", name: "A"), (id: "b", name: "B")],
                 skins: [(name: "Fair", color: (1.0, 0.8, 0.7))], hair: [(name: "Brown", color: (0.4, 0.2, 0.1))],
                 height: (0.9, 1.1)),
                (id: "drake", name: "Drake", description: "", faces: [(id: "a", name: "A")],
                 skins: [(name: "Pale", color: (0.9, 0.9, 1.0))], hair: [(name: "Black", color: (0.1, 0.1, 0.1))],
                 features: [(id: "swept", name: "Swept back"), (id: "curled", name: "Curled")],
                 feature_colors: [(name: "Slate", color: (0.2, 0.2, 0.3))],
                 height: (1.0, 1.2)),
            ])"#,
        )
        .unwrap()
    }

    fn rules() -> AccountRules {
        AccountRules {
            max_characters: 8,
            character_name: (2, 16),
            account_name: (3, 16),
            password_min: 4,
        }
    }

    #[test]
    fn choices_must_exist_for_the_race() {
        let races = races();
        let mut look = Appearance::first(&races);
        assert_eq!(look.check(&races), Ok(()));
        look.face = 1;
        assert_eq!(look.check(&races), Ok(()));
        look.face = 2;
        assert!(look.check(&races).is_err());
        look.face = 0;
        look.feature = 1;
        assert!(look.check(&races).is_err(), "humans have no features");
        look.race = "drake".into();
        assert_eq!(look.check(&races), Ok(()));
        look.race = "goblin".into();
        assert!(look.check(&races).is_err());
    }

    #[test]
    fn height_is_within_the_race_range() {
        let races = races();
        let mut look = Appearance::first(&races);
        look.race = "drake".into();
        look.height = 0.0;
        assert_eq!(look.height_scale(&races), 1.0);
        look.height = 1.0;
        assert!((look.height_scale(&races) - 1.2).abs() < 1e-6);
        look.height = 1.5;
        assert!(look.check(&races).is_err());
    }

    #[test]
    fn character_names_are_tidied_and_checked() {
        let rules = rules();
        assert_eq!(
            rules.character_name("  ari   lanternborn "),
            Ok("Ari Lanternborn".into())
        );
        assert_eq!(rules.character_name("o'reilly"), Ok("O'reilly".into()));
        assert!(rules.character_name("a").is_err());
        assert!(rules.character_name("x1").is_err());
        assert!(rules.character_name("-ari").is_err());
        assert!(rules.character_name("abcdefghijklmnopq").is_err());
    }

    #[test]
    fn account_names_and_passwords_are_checked() {
        let rules = rules();
        assert_eq!(rules.account_name(" sand_flame "), Ok("sand_flame".into()));
        assert!(rules.account_name("ab").is_err());
        assert!(rules.account_name("has space").is_err());
        assert!(rules.password("abc").is_err());
        assert!(rules.password("abcd").is_ok());
    }
}
