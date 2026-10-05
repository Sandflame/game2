//! Accounts and the character list: register, log in, make and delete
//! characters. The slow parts (password hashing, the save file) run on the
//! database thread; this checks requests first and turns the answers into
//! events. Without a save file (tests, demos) there are no accounts and
//! anyone can join with any name.

use std::collections::HashMap;

use bevy::prelude::*;
use shared::components::PlayerId;
use shared::gamedata::{GameData, Zones};
use shared::protocol::{ClientRequest, Link, ServerEvent};

use crate::database::{AccountJob, AccountReply, Database, NewCharacterOwned};

/// Who is logged in: player → (account id, account name).
#[derive(Resource, Default)]
pub struct Sessions(pub HashMap<PlayerId, (i64, String)>);

/// Account requests waiting to be checked and sent to the database thread.
#[derive(Resource, Default)]
pub struct AccountRequests(pub Vec<(PlayerId, ClientRequest)>);

impl AccountRequests {
    /// Keep this request if it is about accounts. Returns whether it was.
    pub fn take(&mut self, player: PlayerId, request: &ClientRequest) -> bool {
        let account = matches!(
            request,
            ClientRequest::Register { .. }
                | ClientRequest::Login { .. }
                | ClientRequest::Logout
                | ClientRequest::CreateCharacter { .. }
                | ClientRequest::DeleteCharacter { .. }
        );
        if account {
            self.0.push((player, request.clone()));
        }
        account
    }
}

fn refuse(link: &mut Link, player: PlayerId, message: impl Into<String>) {
    link.to_client.push(ServerEvent::AccountError {
        player,
        message: message.into(),
    });
}

pub fn handle_account_requests(
    data: Res<GameData>,
    zones: Res<Zones>,
    database: Option<Res<Database>>,
    mut requests: ResMut<AccountRequests>,
    mut sessions: ResMut<Sessions>,
    mut link: ResMut<Link>,
) {
    let rules = &data.accounts;
    for (player, request) in std::mem::take(&mut requests.0) {
        let Some(database) = &database else {
            refuse(
                &mut link,
                player,
                "Accounts need a save file, and there isn't one.",
            );
            continue;
        };
        let logged_in = sessions.0.get(&player).map(|(id, _)| *id);
        match request {
            ClientRequest::Register { account, password }
            | ClientRequest::Login { account, password }
                if logged_in.is_some() =>
            {
                let _ = (account, password);
                refuse(&mut link, player, "You are already logged in.");
            }
            ClientRequest::Register { account, password } => {
                match (rules.account_name(&account), rules.password(&password)) {
                    (Ok(name), Ok(())) => {
                        database.account(player, AccountJob::Register { name, password });
                    }
                    (Err(why), _) | (_, Err(why)) => refuse(&mut link, player, why),
                }
            }
            ClientRequest::Login { account, password } => {
                database.account(
                    player,
                    AccountJob::Login {
                        name: account.trim().to_owned(),
                        password,
                    },
                );
            }
            ClientRequest::Logout => {
                sessions.0.remove(&player);
            }
            ClientRequest::CreateCharacter {
                name,
                class,
                appearance,
            } => {
                let Some(account) = logged_in else {
                    refuse(&mut link, player, "Log in first.");
                    continue;
                };
                let name = match rules.character_name(&name) {
                    Ok(name) => name,
                    Err(why) => {
                        refuse(&mut link, player, why);
                        continue;
                    }
                };
                if !data.classes.contains_key(&class) {
                    refuse(
                        &mut link,
                        player,
                        format!("There is no class called {class}."),
                    );
                    continue;
                }
                if let Err(why) = appearance.check(&data.races) {
                    refuse(&mut link, player, why);
                    continue;
                }
                let start = &data.player.start_zone;
                let (position, yaw) = zones
                    .get(start)
                    .map_or((Vec3::ZERO, 0.0), |l| (l.spawn_point, l.spawn_yaw));
                database.account(
                    player,
                    AccountJob::Create {
                        account,
                        new: NewCharacterOwned {
                            name,
                            class,
                            appearance,
                            zone: start.clone(),
                            position,
                            yaw,
                        },
                        max: rules.max_characters,
                    },
                );
            }
            ClientRequest::DeleteCharacter { name } => match logged_in {
                Some(account) => database.account(player, AccountJob::Delete { account, name }),
                None => refuse(&mut link, player, "Log in first."),
            },
            _ => {}
        }
    }
}

/// Answers from the database thread become events.
pub fn finish_account_jobs(
    database: Option<Res<Database>>,
    mut sessions: ResMut<Sessions>,
    mut link: ResMut<Link>,
) {
    let Some(database) = database else {
        return;
    };
    for (player, reply) in database.take_replies() {
        let event = match reply {
            AccountReply::LoggedIn { account, name } => {
                sessions.0.insert(player, (account, name.clone()));
                ServerEvent::LoggedIn {
                    player,
                    account: name,
                }
            }
            AccountReply::Characters(characters) => ServerEvent::Characters { player, characters },
            AccountReply::Failed(message) => ServerEvent::AccountError { player, message },
        };
        link.to_client.push(event);
    }
}
