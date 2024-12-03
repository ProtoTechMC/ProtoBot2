use crate::pterodactyl::{send_command_safe, PterodactylServerCategory};
use crate::{config, ProtobotData};
use futures::future::try_join_all;
use git_version::git_version;
use log::{error, info};
use serde::de::value::StrDeserializer;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::future::Future;
use uuid::Uuid;

pub(crate) async fn run(
    data: &ProtobotData,
    mut args: impl Iterator<Item = &str>,
) -> Result<(), crate::Error> {
    let Some(operation) = args.next() else {
        print_usage();
        return Ok(());
    };
    match operation {
        "add" => {
            let Some(player) = args.next() else {
                print_usage();
                return Ok(());
            };
            let Some(category) = args.next() else {
                print_usage();
                return Ok(());
            };
            whitelist_across_categories(category, |category| whitelist_add(data, player, category))
                .await?;
        }
        "remove" => {
            let Some(player) = args.next() else {
                print_usage();
                return Ok(());
            };
            let Some(category) = args.next() else {
                print_usage();
                return Ok(());
            };
            whitelist_across_categories(category, |category| {
                whitelist_remove(data, player, category)
            })
            .await?;
        }
        "list" => {
            let Some(category) = args.next() else {
                print_usage();
                return Ok(());
            };
            let Ok(category) = PterodactylServerCategory::deserialize(StrDeserializer::<
                serde_json::Error,
            >::new(category)) else {
                error!("Unknown category {}", category);
                return Ok(());
            };
            if !category.is_minecraft() {
                error!("Can only whitelist on Minecraft servers");
                return Ok(());
            }
            whitelist_list(data, category).await?;
        }
        _ => {
            print_usage();
        }
    }
    Ok(())
}

async fn whitelist_across_categories<F, Fut>(
    category: &str,
    mut whitelist_operation: F,
) -> Result<(), crate::Error>
where
    F: FnMut(PterodactylServerCategory) -> Fut,
    Fut: Future<Output = Result<(), crate::Error>>,
{
    if category == "all" {
        let categories: BTreeSet<_> = config::get()
            .pterodactyl_servers
            .iter()
            .map(|server| server.category)
            .filter(PterodactylServerCategory::is_minecraft)
            .collect();
        try_join_all(categories.into_iter().map(whitelist_operation)).await?;
    } else {
        let Ok(category) = PterodactylServerCategory::deserialize(StrDeserializer::<
            serde_json::Error,
        >::new(category)) else {
            error!("Unknown category {}", category);
            return Ok(());
        };
        if !category.is_minecraft() {
            error!("Can only whitelist on Minecraft servers");
            return Ok(());
        }
        whitelist_operation(category).await?;
    }
    Ok(())
}

async fn whitelist_add(
    data: &ProtobotData,
    player_name: &str,
    category: PterodactylServerCategory,
) -> Result<(), crate::Error> {
    let Some(mut whitelist) = get_whitelist(data, category).await? else {
        return Ok(());
    };

    if whitelist
        .iter()
        .any(|player| player.name.eq_ignore_ascii_case(player_name))
    {
        error!("That player was already whitelisted");
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .user_agent(format!("protobot {}", git_version!()))
        .build()?;
    let response = client
        .post("https://api.minecraftservices.com/minecraft/profile/lookup/bulk/byname")
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .json(&vec![player_name])
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        error!(
            "Failed to request UUID: {}, {}",
            status,
            response.text().await?
        );
        return Ok(());
    }

    #[derive(Deserialize)]
    struct MojangPlayer {
        id: Uuid,
        name: String,
    }

    let mojang_players = response.json::<Vec<MojangPlayer>>().await?;
    if mojang_players.len() != 1 {
        return Err(crate::Error::Other(
            "Mojang server didn't return 1 player when requested".to_owned(),
        ));
    }
    let player_uuid = mojang_players[0].id;
    let player_name: &str = &mojang_players[0].name;

    whitelist.push(Player {
        name: player_name.to_owned(),
        uuid: player_uuid,
    });
    whitelist.sort_by_key(|player| player.name.to_ascii_lowercase());

    set_whitelist(data, whitelist, category, |server| {
        format!("Whitelisted {player_name} on {server}")
    })
    .await?;
    if category.should_be_opped() {
        run_command(data, format!("op {player_name}"), category, |server| {
            format!("Opped {player_name} on {server}")
        })
        .await?;
    }

    Ok(())
}

async fn whitelist_remove(
    data: &ProtobotData,
    player_name: &str,
    category: PterodactylServerCategory,
) -> Result<(), crate::Error> {
    let Some(mut whitelist) = get_whitelist(data, category).await? else {
        return Ok(());
    };

    let mut removed = false;
    whitelist.retain(|player| {
        let matches = player.name.eq_ignore_ascii_case(player_name);
        if matches {
            removed = true;
        }
        !matches
    });
    if !removed {
        error!("That player was not whitelisted");
        return Ok(());
    };

    set_whitelist(data, whitelist, category, |server| {
        format!("Unwhitelisted {player_name} on {server}")
    })
    .await?;
    if category.should_be_opped() {
        run_command(data, format!("deop {player_name}"), category, |server| {
            format!("De-opped {player_name} on {server}")
        })
        .await?;
    }

    Ok(())
}

async fn whitelist_list(
    data: &ProtobotData,
    category: PterodactylServerCategory,
) -> Result<(), crate::Error> {
    let Some(whitelist) = get_whitelist(data, category).await? else {
        return Ok(());
    };
    let mut whitelist: Vec<_> = whitelist.into_iter().map(|player| player.name).collect();
    if whitelist.is_empty() {
        info!("There are no players on the whitelist");
    } else {
        whitelist.sort();
        let num_players = whitelist.len();
        info!(
            "There are {} players on the whitelist: {}",
            num_players,
            whitelist.join(", ")
        );
    }
    Ok(())
}

async fn get_whitelist(
    data: &ProtobotData,
    category: PterodactylServerCategory,
) -> Result<Option<Vec<Player>>, crate::Error> {
    let config = config::get();
    let Some(server) = config.pterodactyl_servers(category).next() else {
        error!("No servers of the given category");
        return Ok(None);
    };
    let whitelist_json = data
        .pterodactyl
        .get_server(&server.id)
        .file_contents_text("whitelist.json")
        .await?;
    let whitelist = serde_json::from_str(&whitelist_json)?;
    Ok(Some(whitelist))
}

async fn set_whitelist(
    data: &ProtobotData,
    whitelist: Vec<Player>,
    category: PterodactylServerCategory,
    mut message: impl FnMut(&str) -> String,
) -> Result<(), crate::Error> {
    let config = config::get();
    let whitelist_json = serde_json::to_string_pretty(&whitelist)?;
    let tasks = config.pterodactyl_servers(category).map(|server| {
        let whitelist_json = whitelist_json.clone();
        let message = message(&server.name);
        async move {
            let ptero_server = data.pterodactyl.get_server(&server.id);
            ptero_server
                .write_file("whitelist.json", whitelist_json)
                .await?;
            send_command_safe(&ptero_server, "whitelist reload").await?;
            info!("{}", message);
            Ok::<(), crate::Error>(())
        }
    });
    futures::future::try_join_all(tasks).await?;
    Ok(())
}

async fn run_command(
    data: &ProtobotData,
    command: String,
    category: PterodactylServerCategory,
    mut message: impl FnMut(&str) -> String,
) -> Result<(), crate::Error> {
    let config = config::get();
    let tasks = config.pterodactyl_servers(category).map(|server| {
        let command = command.clone();
        let message = message(&server.name);
        async move {
            let ptero_server = data.pterodactyl.get_server(&server.id);
            send_command_safe(&ptero_server, command).await?;
            info!("{}", message);
            Ok::<(), crate::Error>(())
        }
    });
    futures::future::try_join_all(tasks).await?;
    Ok(())
}

fn print_usage() {
    info!("(whitelist <add|remove> <player> <category|all>) | (whitelist list <category>)");
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
struct Player {
    name: String,
    uuid: Uuid,
}
