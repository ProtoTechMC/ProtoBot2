mod activity_tracker;
mod april_fools_channel;
mod brainfuck;
mod chess;
mod commands;
mod context;
mod counter;
mod guild_storage;
mod mood;
mod permanent_latest;
mod reaction_role_toggle;
mod role;
mod roletoggle;
mod social_credit;
mod storage;
mod support;
mod update_copy;
mod welcome_message;

use crate::config;
use crate::discord_bot::activity_tracker::ActivityTracker;
use crate::discord_bot::april_fools_channel::{
    get_april_fools_channel, AprilFoolsChannel, AprilFoolsMessageContext,
};
use crate::discord_bot::guild_storage::GuildStorage;
use crate::pterodactyl::{tellraw, PterodactylChatBridge, PterodactylServer};
pub(crate) use context::Context;
use futures::future::try_join_all;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tracing::{error, info, warn};
use twilight_cache_inmemory::{InMemoryCache, ResourceType};
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt};
use twilight_http::Client;
use twilight_model::application::command::CommandType;
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::Message;
use twilight_model::gateway::payload::incoming::{MemberAdd, MemberRemove};
use twilight_model::http::attachment::Attachment;
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::id::Id;
use twilight_util::builder::command::CommandBuilder;
use twilight_util::builder::InteractionResponseDataBuilder;

pub(crate) type Handle = Arc<Client>;

pub(crate) struct DiscordBot {
    pub ctx: Context,
    shard: Shard,
    handler: Handler,
}

struct Handler {
    pterodactyl: Arc<pterodactyl_api::client::Client>,
    own_id: OnceLock<Id<UserMarker>>,
}

async fn create_commands(ctx: &Context, guild_id: Id<GuildMarker>) -> crate::Result<()> {
    ctx.http
        .interaction(ctx.application_id)
        .set_guild_commands(
            guild_id,
            &[
                CommandBuilder::new("hello", "A test command", CommandType::ChatInput).build(),
                CommandBuilder::new(
                    "update_copy",
                    "Updates the SMP copy",
                    CommandType::ChatInput,
                )
                .build(),
            ],
        )
        .await?;
    Ok(())
}

async fn respond_to_interaction(
    ctx: &Context,
    interaction: &Interaction,
    content: &str,
) -> crate::Result<()> {
    ctx.http
        .interaction(ctx.application_id)
        .create_response(
            interaction.id,
            &interaction.token,
            &InteractionResponse {
                kind: InteractionResponseType::ChannelMessageWithSource,
                data: Some(
                    InteractionResponseDataBuilder::new()
                        .content(content)
                        .build(),
                ),
            },
        )
        .await?;
    Ok(())
}

pub(crate) async fn edit_interaction_response(
    ctx: &Context,
    interaction: &Interaction,
    content: &str,
) -> crate::Result<()> {
    ctx.http
        .interaction(ctx.application_id)
        .update_response(&interaction.token)
        .content(Some(content))
        .await?;
    Ok(())
}

#[allow(clippy::single_match)]
async fn process_command(
    ctx: &Context,
    interaction: &Interaction,
    pterodactyl: &pterodactyl_api::client::Client,
) -> crate::Result<()> {
    let Some(InteractionData::ApplicationCommand(command)) = &interaction.data else {
        return Ok(());
    };
    match &command.name[..] {
        "hello" => {
            respond_to_interaction(ctx, interaction, "Hello World!").await?;
        }
        "update_copy" => {
            if interaction.member.as_ref().map(|member| {
                member
                    .roles
                    .contains(&config::get().special_roles.panel_access)
            }) != Some(true)
            {
                respond_to_interaction(
                    ctx,
                    interaction,
                    "You do not have permission to use that command",
                )
                .await?;
                return Ok(());
            }
            respond_to_interaction(ctx, interaction, "Updating copy...").await?;
            match update_copy::run(ctx, interaction, pterodactyl).await {
                Err(err @ (crate::Error::DiscordHttp(_) | crate::Error::DiscordDeserialize(_))) => {
                    return Err(err)
                }
                Err(err) => {
                    edit_interaction_response(
                        ctx,
                        interaction,
                        &format!("Error updating copy: {}", err),
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

async fn process_chatbridge(
    ctx: Context,
    pterodactyl: &pterodactyl_api::client::Client,
    chat_bridge: &PterodactylChatBridge,
    new_message: &Message,
) -> crate::Result<()> {
    let config = config::get();
    try_join_all(
        chat_bridge
            .ptero_servers
            .iter()
            .filter_map(|server_name| {
                config
                    .pterodactyl_servers
                    .iter()
                    .find(|server| &server.name == server_name)
            })
            .map(|server| send_chatbridge_message(&ctx, pterodactyl, server, new_message)),
    )
    .await?;
    try_join_all(
        chat_bridge
            .discord_channels
            .iter()
            .filter(|channel| channel.id != new_message.channel_id)
            .map(|channel| send_chatbridge_message_to_discord(&ctx, &channel.webhook, new_message)),
    )
    .await?;
    Ok(())
}

async fn send_chatbridge_message(
    ctx: &Context,
    pterodactyl: &pterodactyl_api::client::Client,
    server: &PterodactylServer,
    message: &Message,
) -> crate::Result<()> {
    let ptero_server = pterodactyl.get_server(&server.id);

    let sanitized_message = context::content_safe(&ctx.cache, message);
    if !sanitized_message.is_empty() {
        tellraw(
            &ptero_server,
            format!("[Discord] [{}] {}", message.author.name, sanitized_message),
        )
        .await?;
    }

    if !message.attachments.is_empty() {
        let attachment_message = if message.attachments.len() == 1 {
            "an attachment"
        } else {
            "multiple attachments"
        };
        tellraw(
            &ptero_server,
            format!(
                "{} posted {} in Discord.",
                message.author.name, attachment_message
            ),
        )
        .await?;
    }

    Ok(())
}

async fn send_chatbridge_message_to_discord(
    ctx: &Context,
    webhook: &str,
    message: &Message,
) -> crate::Result<()> {
    let client = reqwest::Client::new();
    let attachments = try_join_all(message.attachments.iter().enumerate().map(
        |(id, attachment)| {
            let client = &client;
            async move {
                let file = client
                    .get(&attachment.url)
                    .send()
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?;
                Ok::<_, crate::Error>(Attachment::from_bytes(
                    attachment.filename.clone(),
                    file.to_vec(),
                    id as u64,
                ))
            }
        },
    ))
    .await?;
    let avatar_url = context::avatar_url(&message.author)
        .unwrap_or_else(|| context::default_avatar_url(&message.author));
    execute_webhook(
        &ctx.http,
        webhook,
        &message.content,
        &message.author.name,
        Some(&avatar_url),
        &attachments,
    )
    .await
}

pub(crate) async fn execute_webhook(
    http: &Client,
    webhook_url: &str,
    content: &str,
    username: &str,
    avatar_url: Option<&str>,
    attachments: &[Attachment],
) -> crate::Result<()> {
    let (webhook_id, token) = twilight_util::link::webhook::parse(webhook_url)?;
    let Some(token) = token else {
        return Err(crate::Error::Other(format!(
            "Webhook URL for webhook {webhook_id} is missing a token"
        )));
    };
    let mut request = http
        .execute_webhook(webhook_id, token)
        .content(content)
        .username(username)
        .attachments(attachments);
    if let Some(avatar_url) = avatar_url {
        request = request.avatar_url(avatar_url);
    }
    request.await?;
    Ok(())
}

impl Handler {
    fn own_id(&self) -> Option<Id<UserMarker>> {
        self.own_id.get().copied()
    }

    async fn handle_event(&self, ctx: Context, event: Event, removed_member_name: Option<String>) {
        match event {
            Event::MemberAdd(member_add) => self.guild_member_addition(ctx, *member_add).await,
            Event::MemberRemove(member_remove) => {
                self.guild_member_removal(ctx, member_remove, removed_member_name)
                    .await
            }
            Event::MessageCreate(message) => self.message(ctx, message.0).await,
            Event::MessageUpdate(message) => self.message_update(ctx, message.0).await,
            Event::ReactionAdd(reaction) => {
                reaction_role_toggle::on_reaction_change(ctx, reaction.0, false).await
            }
            Event::ReactionRemove(reaction) => {
                reaction_role_toggle::on_reaction_change(ctx, reaction.0, true).await
            }
            Event::Ready(_) => self.ready(ctx).await,
            Event::InteractionCreate(interaction) => {
                self.interaction_create(ctx, interaction.0).await
            }
            _ => {}
        }
    }

    async fn guild_member_addition(&self, ctx: Context, new_member: MemberAdd) {
        let storage = GuildStorage::get(new_member.guild_id).await;

        if let Some(join_log_channel) = storage.join_log_channel {
            if let Err(err) = ctx
                .send_message(
                    join_log_channel,
                    format!(
                        "{} has risen from the dead.",
                        context::member_display_name(&new_member.member)
                    ),
                )
                .await
            {
                error!("Failed to send message in join log channel: {}", err);
            }
        }

        if let Some(welcome_message_data) = storage.welcome_message.clone() {
            tokio::runtime::Handle::current().spawn(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if let Err(err) = ctx
                    .send_message(
                        welcome_message_data.channel,
                        welcome_message_data
                            .message
                            .replace("[user]", &format!("<@{}>", new_member.user.id)),
                    )
                    .await
                {
                    error!("Failed to send welcome message: {}", err);
                }
            });
        }
    }

    async fn guild_member_removal(
        &self,
        ctx: Context,
        member_remove: MemberRemove,
        removed_member_name: Option<String>,
    ) {
        if let Some(join_log_channel) = GuildStorage::get(member_remove.guild_id)
            .await
            .join_log_channel
        {
            if let Err(err) = ctx
                .send_message(
                    join_log_channel,
                    format!(
                        "{} has been abducted by alien forces.",
                        removed_member_name.unwrap_or(member_remove.user.name)
                    ),
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
        let Some(own_id) = self.own_id() else {
            return;
        };

        {
            let ctx = ctx.clone();
            let new_message = new_message.clone();
            tokio::runtime::Handle::current().spawn(async move {
                if let Err(err) = ActivityTracker::instance()
                    .on_message(own_id, guild_id, ctx, new_message)
                    .await
                {
                    error!("Error processing activity tracker: {}", err);
                }
            });
        }

        enum MessageHandling<'a> {
            ChatBridge(&'a PterodactylChatBridge),
            Command(&'a str),
            IncCounter(&'a str),
            PermanentLatest,
            AprilFools(&'static dyn AprilFoolsChannel),
        }

        let config = config::get();

        let message_handling = {
            if let Some(april_fools_channel) =
                get_april_fools_channel(guild_id, new_message.channel_id).await
            {
                MessageHandling::AprilFools(april_fools_channel)
            } else if new_message.author.bot {
                return;
            } else if let Some(chat_bridge) =
                config.chat_bridge_by_discord_channel(new_message.channel_id)
            {
                MessageHandling::ChatBridge(chat_bridge)
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
                            } else if let Some(counter) = new_message.content.strip_suffix("++") {
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
            MessageHandling::ChatBridge(chatbridge_server) => {
                process_chatbridge(ctx, &self.pterodactyl, chatbridge_server, &new_message).await
            }
            MessageHandling::Command(command) => {
                commands::run(command, guild_id, ctx, &new_message).await
            }
            MessageHandling::IncCounter(counter) => {
                counter::inc_counter(counter, guild_id, ctx, &new_message).await
            }
            MessageHandling::PermanentLatest => {
                permanent_latest::on_message(guild_id, ctx, &new_message).await
            }
            MessageHandling::AprilFools(april_fools) => {
                april_fools_channel::on_message(
                    april_fools,
                    AprilFoolsMessageContext {
                        context: ctx,
                        has_attachments: !new_message.attachments.is_empty(),
                        content: &new_message.content,
                        author: &new_message.author,
                        guild_id,
                        channel_id: new_message.channel_id,
                        message_id: new_message.id,
                        own_id,
                    },
                )
                .await
            }
        } {
            warn!(
                "Error processing message from \"{}\" (ID {}): {}",
                new_message.author.name, new_message.author.id, err
            );
        }
    }

    async fn message_update(&self, ctx: Context, message: Message) {
        let Some(own_id) = self.own_id() else {
            return;
        };

        let Some(guild_id) = message.guild_id else {
            return;
        };

        enum MessageEditHandling {
            AprilFools(&'static dyn AprilFoolsChannel),
        }
        let handling = if let Some(april_fools_channel) =
            get_april_fools_channel(guild_id, message.channel_id).await
        {
            MessageEditHandling::AprilFools(april_fools_channel)
        } else {
            return;
        };
        if let Err(err) = match handling {
            MessageEditHandling::AprilFools(april_fools) => {
                april_fools_channel::on_message(
                    april_fools,
                    AprilFoolsMessageContext {
                        context: ctx,
                        has_attachments: !message.attachments.is_empty(),
                        content: &message.content,
                        author: &message.author,
                        guild_id,
                        channel_id: message.channel_id,
                        message_id: message.id,
                        own_id,
                    },
                )
                .await
            }
        } {
            warn!(
                "Error processing message edit from \"{}\" (ID {}): {}",
                message.author.name, message.author.id, err
            );
        }
    }

    async fn ready(&self, ctx: Context) {
        if let Err(err) = create_commands(&ctx, config::get().guild_id).await {
            error!("Failed to register commands: {}", err);
            return;
        }

        info!("Discord bot ready");
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Err(err) = process_command(&ctx, &interaction, &self.pterodactyl).await {
            error!("Failed to process command: {}", err);
        }
    }
}

pub(crate) async fn create_client(
    pterodactyl: Arc<pterodactyl_api::client::Client>,
) -> crate::Result<DiscordBot> {
    let token = config::get().discord_token.clone();
    let intents = Intents::GUILDS
        | Intents::GUILD_MEMBERS
        | Intents::GUILD_MESSAGES
        | Intents::MESSAGE_CONTENT
        | Intents::GUILD_MESSAGE_REACTIONS;

    let http = Arc::new(Client::new(token.clone()));
    let application_id = http.current_user_application().await?.model().await?.id;
    let cache = Arc::new(
        InMemoryCache::builder()
            .resource_types(
                ResourceType::GUILD
                    | ResourceType::CHANNEL
                    | ResourceType::ROLE
                    | ResourceType::MEMBER
                    | ResourceType::USER
                    | ResourceType::USER_CURRENT,
            )
            .build(),
    );

    Ok(DiscordBot {
        ctx: Context {
            http,
            cache,
            application_id,
        },
        shard: Shard::new(ShardId::ONE, token, intents),
        handler: Handler {
            pterodactyl,
            own_id: OnceLock::new(),
        },
    })
}

pub(crate) async fn run(bot: DiscordBot) -> crate::Result<()> {
    let DiscordBot {
        ctx,
        mut shard,
        handler,
    } = bot;
    let handler = Arc::new(handler);

    let event_loop = async {
        while let Some(event) = shard.next_event(EventTypeFlags::all()).await {
            let event = match event {
                Ok(event) => event,
                Err(err) => {
                    warn!("Error receiving discord gateway event: {}", err);
                    continue;
                }
            };

            // the member is removed from the cache on update, so get their name beforehand
            let removed_member_name = match &event {
                Event::MemberRemove(member_remove) => context::cached_display_name(
                    &ctx.cache,
                    member_remove.guild_id,
                    member_remove.user.id,
                ),
                _ => None,
            };
            ctx.cache.update(&event);

            if let Event::Ready(ready) = &event {
                let _ = handler.own_id.set(ready.user.id);
            }

            // handle each event in its own task, so that long-running processing of one event
            // (e.g. a slow command) doesn't hold up the processing of other events
            let ctx = ctx.clone();
            let handler = handler.clone();
            tokio::runtime::Handle::current().spawn(async move {
                handler.handle_event(ctx, event, removed_member_name).await;
            });
        }
    };

    tokio::select! {
        _ = crate::wait_shutdown() => {}
        _ = event_loop => {}
    }
    Ok(())
}
