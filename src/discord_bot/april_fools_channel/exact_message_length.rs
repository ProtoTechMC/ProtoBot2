use crate::discord_bot::april_fools_channel::{AprilFoolsChannel, AprilFoolsMessageContext};
use crate::discord_bot::guild_storage::GuildStorage;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serenity::all::ChannelId;
use serenity::builder::CreateMessage;

pub(crate) struct ExactMessageLength;

pub(crate) static CHANNEL: ExactMessageLength = ExactMessageLength;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub(crate) struct ExactMessageLengthData {
    pub channel: ChannelId,
    #[serde(default)]
    pub expected_length: Option<u16>,
}

#[async_trait]
impl AprilFoolsChannel for ExactMessageLength {
    async fn get_error(&self, context: &AprilFoolsMessageContext<'_>) -> Option<String> {
        let storage = GuildStorage::get(context.guild_id).await;
        let Some(expected_length_data) = &storage.april_fools_channels.exact_message_length else {
            return None;
        };

        if context.author.id == context.own_id
            && !context.has_attachments
            && is_expected_length_message(context.content)
        {
            return None;
        }

        let message_length = context.content.chars().count();

        if let Some(expected_length) = expected_length_data.expected_length {
            if expected_length as usize != message_length {
                return Some(format!("Your message was the wrong length! Expected length {expected_length} but it was {message_length}."));
            }
        }

        None
    }

    async fn on_success(&self, context: &AprilFoolsMessageContext<'_>) -> crate::Result<()> {
        let just_had_67 = GuildStorage::get(context.guild_id)
            .await
            .april_fools_channels
            .exact_message_length
            .as_ref()
            .and_then(|exact_message_length| exact_message_length.expected_length)
            == Some(67);
        let new_length = reroll_message_length(just_had_67);
        context
            .channel_id
            .send_message(
                &context.context,
                CreateMessage::new()
                    .content(format!("Now expecting message length of {new_length}")),
            )
            .await?;
        let mut storage = GuildStorage::get_mut(context.guild_id).await;
        let Some(expected_length_data) = &mut storage.april_fools_channels.exact_message_length
        else {
            storage.discard();
            return Ok(());
        };
        expected_length_data.expected_length = Some(new_length);
        storage.save().await;
        Ok(())
    }
}

fn reroll_message_length(just_had_67: bool) -> u16 {
    if !just_had_67 && rand::random::<bool>() {
        67
    } else {
        let length = rand::random_range(5..=99);
        if length >= 67 {
            length + 1
        } else {
            length
        }
    }
}

fn is_expected_length_message(message: &str) -> bool {
    let Some(expected_length) = message.strip_prefix("Now expecting message length of ") else {
        return false;
    };
    expected_length.parse::<u16>().is_ok()
}
