mod guild_storage;

use crate::config;
use async_trait::async_trait;
use log::{error, info};
use serenity::client::{Context, EventHandler};
use serenity::http::Http;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::{Interaction, InteractionResponseType};
use serenity::model::id::GuildId;
use serenity::prelude::GatewayIntents;
use serenity::Client;
use std::sync::Arc;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use crate::discord_bot::guild_storage::GuildStorage;

pub(crate) type Handle = Arc<Http>;

struct Handler;

async fn create_commands(ctx: &Context, guild_id: GuildId) -> serenity::Result<()> {
    guild_id
        .set_application_commands(&ctx.http, |commands| {
            commands.create_application_command(|command| {
                command.name("hello").description("A test command")
            })
        })
        .await?;
    Ok(())
}

#[allow(clippy::single_match)]
async fn process_command(
    ctx: &Context,
    command: ApplicationCommandInteraction,
) -> serenity::Result<()> {
    match &command.data.name[..] {
        "hello" => {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| message.content("Hello World!"))
                })
                .await?;
        }
        _ => {}
    }
    Ok(())
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, _ctx: Context, new_message: Message) {
        let guild_id = match new_message.guild_id {
            Some(guild_id) => guild_id,
            None => return,
        };
        let command = {
            let storage = GuildStorage::get(guild_id).await;
            match new_message.content.strip_prefix(&storage.command_prefix) {
                Some(content) => content,
                None => return,
            }
        };
        info!("Received discord command {}", command);
    }

    async fn ready(&self, ctx: Context, _data_about_bot: Ready) {
        if let Err(err) = create_commands(&ctx, config::get().guild_id).await {
            error!("Failed to register commands: {}", err);
            return;
        }

        info!("Discord bot ready");
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            if let Err(err) = process_command(&ctx, command).await {
                error!("Failed to process command: {}", err);
            }
        }
    }
}

pub(crate) async fn create_client() -> Result<Client, crate::Error> {
    let intents = GatewayIntents::GUILD_MESSAGES;
    Ok(Client::builder(&config::get().discord_token, intents)
        .event_handler(Handler)
        .await?)
}

pub(crate) async fn run(mut client: Client) -> Result<(), crate::Error> {
    client.start().await?;
    Ok(())
}