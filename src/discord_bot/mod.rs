mod brainfuck;
mod chess;
mod commands;
mod counter;
mod guild_storage;
mod js_messages;
mod mood;
mod permanent_latest;
mod reaction_role_toggle;
mod role;
mod roletoggle;
mod simple_words;
mod storage;
mod support;
mod update_copy;

use crate::config;
use crate::discord_bot::guild_storage::GuildStorage;
use async_trait::async_trait;
use log::{error, info, warn};
use serenity::builder::{
    CreateCommand, CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage,
    EditInteractionResponse,
};
use serenity::client::{Context, EventHandler};
use serenity::http::Http;
use serenity::model::application::{CommandInteraction, Interaction};
use serenity::model::channel::{Message, Reaction};
use serenity::model::event::MessageUpdateEvent;
use serenity::model::gateway::Ready;
use serenity::model::guild::Member;
use serenity::model::id::GuildId;
use serenity::model::user::User;
use serenity::prelude::GatewayIntents;
use serenity::Client;
use std::sync::Arc;

pub(crate) type Handle = Arc<Http>;

struct Handler {
    pterodactyl: Arc<pterodactyl_api::client::Client>,
}

async fn create_commands(ctx: &Context, guild_id: GuildId) -> serenity::Result<()> {
    guild_id
        .set_commands(
            &ctx.http,
            vec![
                CreateCommand::new("hello").description("A test command"),
                CreateCommand::new("update_copy").description("Updates the SMP copy"),
            ],
        )
        .await?;
    Ok(())
}

#[allow(clippy::single_match)]
async fn process_command(
    ctx: &Context,
    command: CommandInteraction,
    pterodactyl: &pterodactyl_api::client::Client,
) -> serenity::Result<()> {
    match &command.data.name[..] {
        "hello" => {
            command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new().content("Hello World!"),
                    ),
                )
                .await?;
        }
        "update_copy" => {
            if command
                .member
                .as_ref()
                .map(|member| member.roles.contains(&config::get().panel_access_role))
                != Some(true)
            {
                command
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("You do not have permission to use that command"),
                        ),
                    )
                    .await?;
                return Ok(());
            }
            command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new().content("Updating copy..."),
                    ),
                )
                .await?;
            match update_copy::run(ctx, &command, pterodactyl).await {
                Err(crate::Error::Serenity(err)) => return Err(err),
                Err(err) => {
                    command
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new()
                                .content(format!("Error updating copy: {}", err)),
                        )
                        .await?;
                }
                Ok(()) => {}
            }
        }
        _ => {}
    }
    Ok(())
}

#[async_trait]
impl EventHandler for Handler {
    async fn guild_member_addition(&self, ctx: Context, new_member: Member) {
        if let Some(join_log_channel) = GuildStorage::get(new_member.guild_id)
            .await
            .join_log_channel
        {
            if let Err(err) = join_log_channel
                .send_message(
                    ctx,
                    CreateMessage::new().content(format!(
                        "{} has risen from the dead.",
                        new_member.display_name()
                    )),
                )
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
                .send_message(
                    ctx,
                    CreateMessage::new().content(format!(
                        "{} has been abducted by alien forces.",
                        member_data_if_available
                            .map(|member| member.display_name().to_owned())
                            .unwrap_or(user.name)
                    )),
                )
                .await
            {
                error!("Failed to send message in join log channel: {}", err);
            }
        }
    }

    async fn message(&self, ctx: Context, new_message: Message) {
        let guild_id = match new_message.guild_id {
            Some(guild_id) => guild_id,
            None => return,
        };

        tokio::runtime::Handle::current().spawn(async move {
            enum MessageHandling<'a> {
                Command(&'a str),
                IncCounter(&'a str),
                PermanentLatest,
                SimpleWords,
                JsChannel,
            }

            let message_handling = {
                if config::get().simple_words_channel == Some(new_message.channel_id) {
                    MessageHandling::SimpleWords
                } else if config::get().js_channel == Some(new_message.channel_id) {
                    MessageHandling::JsChannel
                } else if new_message.author.bot {
                    return;
                } else {
                    let storage = GuildStorage::get(guild_id).await;
                    if storage
                        .permanent_latest
                        .is_permanent_latest_channel(new_message.channel_id)
                    {
                        MessageHandling::PermanentLatest
                    } else {
                        match new_message.content.strip_prefix(&storage.command_prefix) {
                            Some(content) => MessageHandling::Command(content),
                            None => {
                                if let Some(counter) = new_message.content.strip_prefix("++") {
                                    MessageHandling::IncCounter(counter)
                                } else if let Some(counter) = new_message.content.strip_suffix("++")
                                {
                                    MessageHandling::IncCounter(counter)
                                } else {
                                    return;
                                }
                            }
                        }
                    }
                }
            };

            if let Err(err) = match message_handling {
                MessageHandling::Command(command) => {
                    commands::run(command, guild_id, ctx, &new_message).await
                }
                MessageHandling::IncCounter(counter) => {
                    counter::inc_counter(counter, guild_id, ctx, &new_message).await
                }
                MessageHandling::PermanentLatest => {
                    permanent_latest::on_message(guild_id, ctx, &new_message).await
                }
                MessageHandling::JsChannel => {
                    js_messages::on_message(
                        ctx,
                        !new_message.attachments.is_empty(),
                        &new_message.content,
                        &new_message.author,
                        new_message.channel_id,
                        new_message.id,
                    )
                    .await
                }
                MessageHandling::SimpleWords => {
                    simple_words::on_message(
                        ctx,
                        !new_message.attachments.is_empty(),
                        &new_message.content,
                        &new_message.author,
                        new_message.channel_id,
                        new_message.id,
                    )
                    .await
                }
            } {
                warn!(
                    "Error processing message from \"{}\" (ID {}): {}",
                    new_message.author.name, new_message.author.id, err
                );
            }
        });
    }

    async fn message_update(
        &self,
        ctx: Context,
        _old_if_available: Option<Message>,
        _new: Option<Message>,
        event: MessageUpdateEvent,
    ) {
        enum MessageEditHandling {
            SimpleWords,
            JsMessage,
        }
        let handling = if config::get().simple_words_channel == Some(event.channel_id) {
            MessageEditHandling::SimpleWords
        } else if config::get().js_channel == Some(event.channel_id) {
            MessageEditHandling::JsMessage
        } else {
            return;
        };
        let Some(content) = event.content else {
            return;
        };
        let Some(author) = event.author else {
            return;
        };
        tokio::runtime::Handle::current().spawn(async move {
            if let Err(err) = match handling {
                MessageEditHandling::SimpleWords => {
                    simple_words::on_message(
                        ctx,
                        event
                            .attachments
                            .as_ref()
                            .map(|attachments| attachments.is_empty())
                            == Some(false),
                        &content,
                        &author,
                        event.channel_id,
                        event.id,
                    )
                    .await
                }
                MessageEditHandling::JsMessage => {
                    js_messages::on_message(
                        ctx,
                        event
                            .attachments
                            .as_ref()
                            .map(|attachments| attachments.is_empty())
                            == Some(false),
                        &content,
                        &author,
                        event.channel_id,
                        event.id,
                    )
                    .await
                }
            } {
                warn!(
                    "Error processing message edit from \"{}\" (ID {}): {}",
                    author.name, author.id, err
                );
            }
        });
    }

    async fn reaction_add(&self, ctx: Context, reaction: Reaction) {
        reaction_role_toggle::on_reaction_change(ctx, reaction, false).await;
    }

    async fn reaction_remove(&self, ctx: Context, reaction: Reaction) {
        reaction_role_toggle::on_reaction_change(ctx, reaction, true).await;
    }

    async fn ready(&self, ctx: Context, _data_about_bot: Ready) {
        if let Err(err) = create_commands(&ctx, config::get().guild_id).await {
            error!("Failed to register commands: {}", err);
            return;
        }

        info!("Discord bot ready");
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = interaction {
            let pterodactyl = self.pterodactyl.clone();
            tokio::runtime::Handle::current().spawn(async move {
                if let Err(err) = process_command(&ctx, command, &pterodactyl).await {
                    error!("Failed to process command: {}", err);
                }
            });
        }
    }
}

pub(crate) async fn create_client(
    pterodactyl: Arc<pterodactyl_api::client::Client>,
) -> Result<Client, crate::Error> {
    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MEMBERS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_MESSAGE_REACTIONS;
    Ok(Client::builder(&config::get().discord_token, intents)
        .event_handler(Handler { pterodactyl })
        .await?)
}

pub(crate) async fn run(mut client: Client) -> Result<(), crate::Error> {
    tokio::select! {
        _ = crate::wait_shutdown() => {}
        result = client.start() => result?,
    }
    Ok(())
}
