use futures::StreamExt;
use serenity::client::Context;
use serenity::model::id::{ChannelId, MessageId};
use serenity::model::user::User;

pub(crate) async fn on_message(
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

    let Some(Ok(latest_message)) = channel_id.messages_iter(&ctx).boxed().next().await else {
        return Ok(());
    };
    if *author == latest_message.author {
        channel_id.delete_message(&ctx, message_id).await?;
        return Ok(());
    }
    let previous_counter = i32::from_str_radix(&latest_message.content, 8).unwrap();

    if next_counter != previous_counter + 1 {
        channel_id.delete_message(&ctx, message_id).await?;
        return Ok(());
    }

    Ok(())
}
