mod brainfuck;
mod chess;
mod commands;
mod guild_storage;
mod mood;
mod role;
mod storage;

use crate::config;
use crate::discord_bot::guild_storage::GuildStorage;
use async_trait::async_trait;
use log::{error, info, warn};
use serenity::client::{Context, EventHandler};
use serenity::http::Http;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::{Interaction, InteractionResponseType};
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::guild::Member;
use serenity::model::id::GuildId;
use serenity::model::user::User;
use serenity::prelude::GatewayIntents;
use serenity::Client;
use std::sync::Arc;

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
    async fn message(&self, ctx: Context, new_message: Message) {
        if new_message.author.bot {
            return;
        }
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
        if let Err(err) = commands::run(command, guild_id, ctx, &new_message).await {
            warn!("Error executing command: {}", err);
        }
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

    async fn guild_member_addition(&self, ctx: Context, new_member: Member) {
        if let Some(join_log_channel) = GuildStorage::get(new_member.guild_id)
            .await
            .join_log_channel
        {
            if let Err(err) = join_log_channel
                .send_message(ctx, |message| {
                    message.content(format!(
                        "{} has risen from the dead.",
                        new_member.display_name()
                    ))
                })
                .await
            {
                error!("Failed to send message in join log channel: {}", err);
            }
        }
    }

    async fn guild_member_removal(
        &self,
        ctx: Context,
        guild_id: GuildId,
        user: User,
        member_data_if_available: Option<Member>,
    ) {
        if let Some(join_log_channel) = GuildStorage::get(guild_id).await.join_log_channel {
            if let Err(err) = join_log_channel
                .send_message(ctx, move |message| {
                    message.content(format!(
                        "{} has been abducted by alien forces.",
                        member_data_if_available
                            .map(|member| member.display_name().into_owned())
                            .unwrap_or(user.name)
                    ))
                })
                .await
            {
                error!("Failed to send message in join log channel: {}", err);
            }
        }
    }
}

pub(crate) async fn create_client() -> Result<Client, crate::Error> {
    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MEMBERS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;
    Ok(Client::builder(&config::get().discord_token, intents)
        .event_handler(Handler)
        .await?)
}

pub(crate) async fn run(mut client: Client) -> Result<(), crate::Error> {
    client.start().await?;
    Ok(())
}
