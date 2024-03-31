use rslint_errors::Severity;
use rslint_parser::parse_module;
use serenity::client::Context;
use serenity::model::id::{ChannelId, MessageId};
use serenity::model::user::User;

fn is_valid_js(text: &str) -> bool {
    std::panic::catch_unwind(|| {
        parse_module(text, 0)
            .errors()
            .iter()
            .any(|err| err.severity == Severity::Error)
    })
    .unwrap_or(false)
}

pub(crate) async fn on_message(
    ctx: Context,
    has_attachments: bool,
    content: &str,
    author: &User,
    channel_id: ChannelId,
    message_id: MessageId,
) -> Result<(), crate::Error> {
    // find words in message
    let mut error_message = None;
    if has_attachments {
        error_message = Some("Message has a picture".to_owned());
    }
    if error_message.is_none() && !is_valid_js(content) {
        error_message = Some("Message was invalid javascript".to_owned());
    }
    if let Some(error_message) = error_message {
        channel_id.delete_message(&ctx, message_id).await?;
        if !author.bot {
            let dm_channel = author.create_dm_channel(&ctx).await?;
            dm_channel
                .send_message(&ctx, |new_message| {
                    new_message.content(format!("{error_message}! Your original message was:"))
                })
                .await?;
            if !content.is_empty() {
                dm_channel
                    .send_message(ctx, |new_message| new_message.content(content))
                    .await?;
            }
        }
    }
    Ok(())
}
