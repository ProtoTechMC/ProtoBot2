use crate::config;
use serenity::all::{ChannelId, Context, CreateMessage, MessageFlags, MessageId, User};

mod haiku;
mod simple_words;

pub(crate) fn get_april_fools_channel(
    channel_id: ChannelId,
) -> Option<&'static dyn AprilFoolsChannel> {
    let config = config::get();
    let channels = &config.special_channels;
    if channels.simple_words == Some(channel_id) {
        Some(&simple_words::CHANNEL)
    } else if channels.haiku == Some(channel_id) {
        Some(&haiku::CHANNEL)
    } else {
        None
    }
}

pub(crate) async fn on_message(
    ctx: Context,
    april_fools: &(impl AprilFoolsChannel + ?Sized),
    has_attachments: bool,
    content: &str,
    author: &User,
    channel_id: ChannelId,
    message_id: MessageId,
) -> Result<(), crate::Error> {
    // find words in message
    let error_message = if has_attachments {
        Some(april_fools.has_attachment_message().to_owned())
    } else {
        april_fools.get_error(content)
    };
    if let Some(mut error_message) = error_message {
        channel_id.delete_message(&ctx, message_id).await?;
        if !author.bot {
            let dm_channel = author.create_dm_channel(&ctx).await?;
            if let Some(your_original_message_was) = april_fools.your_original_message_was() {
                error_message.push(' ');
                error_message.push_str(your_original_message_was);
            }
            dm_channel
                .send_message(&ctx, CreateMessage::new().content(error_message))
                .await?;
            if !content.is_empty() {
                dm_channel
                    .send_message(
                        ctx,
                        CreateMessage::new()
                            .content(content)
                            .flags(MessageFlags::SUPPRESS_NOTIFICATIONS),
                    )
                    .await?;
            }
        }
    }
    Ok(())
}

pub(crate) trait AprilFoolsChannel: Send + Sync {
    fn get_error(&self, message: &str) -> Option<String>;

    fn your_original_message_was(&self) -> Option<&'static str> {
        Some("Your original message was:")
    }

    fn has_attachment_message(&self) -> &'static str {
        "Message has attachment"
    }
}
