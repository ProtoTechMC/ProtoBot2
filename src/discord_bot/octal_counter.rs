use serde::{Deserialize, Serialize};
use serenity::client::Context;
use serenity::model::id::{ChannelId, GuildId, MessageId, UserId};
use serenity::model::user::User;
use crate::discord_bot::guild_storage::GuildStorage;

pub(crate) async fn on_message(
    guild_id: GuildId,
    ctx: Context,
    has_attachments: bool,
    content: &str,
    author: &User,
    channel_id: ChannelId,
    message_id: MessageId,
) -> Result<(), crate::Error> {
    if has_attachments {
        channel_id.delete_message(&ctx, message_id).await?;
        return Ok(());
    }

    let Ok(next_counter) = i32::from_str_radix(content, 8) else {
        channel_id.delete_message(&ctx, message_id).await?;
        return Ok(());
    };

    let mut storage = GuildStorage::get_mut(guild_id).await;
    if storage.octal_counter_state.octal_counter_latest_user == Some(author.id)  {
        storage.discard();
        channel_id.delete_message(&ctx, message_id).await?;
        return Ok(());
    }
    if next_counter != storage.octal_counter_state.octal_counter + 1 {
        storage.discard();
        channel_id.delete_message(&ctx, message_id).await?;
        return Ok(());
    }

    storage.octal_counter_state.octal_counter = next_counter;
    storage.octal_counter_state.octal_counter_latest_user = Some(author.id);
    storage.save().await;
    Ok(())
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct OctalCounterState {
    pub octal_counter_channel: Option<ChannelId>,
    pub octal_counter: i32,
    pub octal_counter_latest_user: Option<UserId>,
}
