use crate::discord_bot::april_fools_channel::{AprilFoolsChannel, AprilFoolsMessageContext};
use crate::discord_bot::guild_storage::GuildStorage;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId, MessageId};
use serenity::builder::CreateMessage;

pub(crate) struct ExactMessageLength;

pub(crate) static CHANNEL: ExactMessageLength = ExactMessageLength;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub(crate) struct ExactMessageLengthData {
    pub channel: ChannelId,
    #[serde(default)]
    pub expected_length: Option<u16>,
    #[serde(default)]
    pub last_inform_message: Option<MessageId>,
}

#[async_trait]
impl AprilFoolsChannel for ExactMessageLength {
    async fn get_error(&self, context: &AprilFoolsMessageContext<'_>) -> Option<String> {
        let mut storage = GuildStorage::get_mut(context.guild_id).await;
        let Some(expected_length_data) = &mut storage.april_fools_channels.exact_message_length
        else {
            storage.discard();
            return None;
        };

        if expected_length_data.last_inform_message == Some(context.message_id) {
            storage.discard();
            return None;
        }

        let message_length = context.content.chars().count();
        let mut just_had_67 = false;

        if let Some(expected_length) = expected_length_data.expected_length {
            if expected_length as usize != message_length {
                storage.discard();
                return Some(format!("Your message was the wrong length! Expected length {expected_length} but it was {message_length}."));
            }

            just_had_67 = expected_length == 67;
        }

        let new_length = reroll_message_length(just_had_67);
        expected_length_data.expected_length = Some(new_length);
        storage.save().await;
        None
    }

    async fn on_success(&self, context: &AprilFoolsMessageContext<'_>) -> crate::Result<()> {
        let mut storage = GuildStorage::get_mut(context.guild_id).await;
        let Some(expected_length_data) = &mut storage.april_fools_channels.exact_message_length
        else {
            storage.discard();
            return Ok(());
        };
        let Some(expected_length) = expected_length_data.expected_length else {
            storage.discard();
            return Ok(());
        };
        let message = match context
            .channel_id
            .send_message(
                &context.context,
                CreateMessage::new()
                    .content(format!("Now expecting message length of {expected_length}")),
            )
            .await
        {
            Ok(message) => message,
            Err(err) => {
                storage.discard();
                return Err(err.into());
            }
        };
        expected_length_data.last_inform_message = Some(message.id);
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
