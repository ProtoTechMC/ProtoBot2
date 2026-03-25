use crate::discord_bot::april_fools_channel::exact_message_length::ExactMessageLengthData;
use crate::discord_bot::guild_storage::GuildStorage;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId, Context, CreateMessage, GuildId, MessageFlags, MessageId, User};

mod exact_message_length;
mod haiku;
mod simple_words;

#[derive(Debug)]
pub(crate) struct AprilFoolsMessageContext<'a> {
    pub context: Context,
    pub has_attachments: bool,
    pub content: &'a str,
    pub author: &'a User,
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    pub message_id: MessageId,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub(crate) struct AprilFoolsChannels {
    #[serde(default)]
    pub simple_words: Option<ChannelId>,
    #[serde(default)]
    pub haiku: Option<ChannelId>,
    #[serde(default)]
    pub exact_message_length: Option<ExactMessageLengthData>,
}

pub(crate) async fn get_april_fools_channel(
    guild_id: GuildId,
    channel_id: ChannelId,
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
            .channel_id
            .delete_message(&context.context, context.message_id)
            .await?;
        if !context.author.bot {
            let dm_channel = context.author.create_dm_channel(&context.context).await?;
            if let Some(your_original_message_was) = april_fools.your_original_message_was() {
                error_message.push(' ');
                error_message.push_str(your_original_message_was);
            }
            dm_channel
                .send_message(
                    &context.context,
                    CreateMessage::new().content(error_message),
                )
                .await?;
            if !context.content.is_empty() {
                dm_channel
                    .send_message(
                        context.context,
                        CreateMessage::new()
                            .content(context.content)
                            .flags(MessageFlags::SUPPRESS_NOTIFICATIONS),
                    )
                    .await?;
            }
        }
    } else {
        april_fools.on_success(&context).await?;
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
