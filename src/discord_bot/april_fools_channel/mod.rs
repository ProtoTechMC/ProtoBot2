use crate::discord_bot::april_fools_channel::exact_message_length::ExactMessageLengthData;
use crate::discord_bot::context::Context;
use crate::discord_bot::guild_storage::GuildStorage;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use twilight_model::channel::message::MessageFlags;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker};
use twilight_model::id::Id;
use twilight_model::user::User;

mod exact_message_length;
mod haiku;
mod simple_words;

#[derive(Debug)]
pub(crate) struct AprilFoolsMessageContext<'a> {
    pub context: Context,
    pub has_attachments: bool,
    pub content: &'a str,
    pub author: &'a User,
    pub guild_id: Id<GuildMarker>,
    pub channel_id: Id<ChannelMarker>,
    pub message_id: Id<MessageMarker>,
    pub own_id: Id<UserMarker>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub(crate) struct AprilFoolsChannels {
    #[serde(default)]
    pub simple_words: Option<Id<ChannelMarker>>,
    #[serde(default)]
    pub haiku: Option<Id<ChannelMarker>>,
    #[serde(default)]
    pub exact_message_length: Option<ExactMessageLengthData>,
}

pub(crate) async fn get_april_fools_channel(
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
) -> Option<&'static dyn AprilFoolsChannel> {
    let storage = GuildStorage::get(guild_id).await;
    let channels = &storage.april_fools_channels;
    if channels.simple_words == Some(channel_id) {
        Some(&simple_words::CHANNEL)
    } else if channels.haiku == Some(channel_id) {
        Some(&haiku::CHANNEL)
    } else if channels
        .exact_message_length
        .as_ref()
        .is_some_and(|exact_message_length| exact_message_length.channel == channel_id)
    {
        Some(&exact_message_length::CHANNEL)
    } else {
        None
    }
}

pub(crate) async fn on_message(
    april_fools: &(impl AprilFoolsChannel + ?Sized),
    context: AprilFoolsMessageContext<'_>,
) -> crate::Result<()> {
    // find words in message
    let error_message = if context.has_attachments {
        Some(april_fools.has_attachment_message().to_owned())
    } else {
        april_fools.get_error(&context).await
    };
    if let Some(mut error_message) = error_message {
        context
            .context
            .http
            .delete_message(context.channel_id, context.message_id)
            .await?;
        if !context.author.bot {
            let dm_channel = context.context.create_dm_channel(context.author.id).await?;
            if let Some(your_original_message_was) = april_fools.your_original_message_was() {
                error_message.push(' ');
                error_message.push_str(your_original_message_was);
            }
            context
                .context
                .send_message(dm_channel.id, error_message)
                .await?;
            if !context.content.is_empty() {
                context
                    .context
                    .http
                    .create_message(dm_channel.id)
                    .content(context.content)
                    .flags(MessageFlags::SUPPRESS_NOTIFICATIONS)
                    .await?;
            }
        }
    } else {
        if !context.author.bot {
            april_fools.on_success(&context).await?;
        }
    }
    Ok(())
}

#[async_trait]
pub(crate) trait AprilFoolsChannel: Send + Sync {
    async fn get_error(&self, context: &AprilFoolsMessageContext<'_>) -> Option<String>;

    fn your_original_message_was(&self) -> Option<&'static str> {
        Some("Your original message was:")
    }

    fn has_attachment_message(&self) -> &'static str {
        "Message has attachment"
    }

    async fn on_success(&self, _context: &AprilFoolsMessageContext<'_>) -> crate::Result<()> {
        Ok(())
    }
}
