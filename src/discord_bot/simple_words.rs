use crate::config;
use lazy_static::lazy_static;
use serenity::client::Context;
use serenity::model::channel::Message;
use std::collections::HashSet;

lazy_static! {
    static ref TOP_10K_WORDS: HashSet<&'static str> = {
        let mut set = HashSet::with_capacity(config::get().num_simple_words);
        for word in include!("top_10k_words.txt")
            .into_iter()
            .take(config::get().num_simple_words)
        {
            set.insert(word);
        }
        set
    };
}

pub(crate) async fn on_message(ctx: Context, message: &Message) -> Result<(), crate::Error> {
    // find words in message
    let mut error_message = None;
    if !message.attachments.is_empty() {
        error_message = Some("Message has an image");
    } else {
        let mut current_word_start = None;
        for (index, char) in message.content.char_indices() {
            if !char.is_ascii() {
                error_message = Some("Message has a hard thing");
                break;
            }
            if char.is_ascii_alphabetic() {
                if current_word_start.is_none() {
                    current_word_start = Some(index);
                }
            } else if let Some(word_start) = current_word_start {
                current_word_start = None;
                let word = &message.content[word_start..index];
                if !is_word_allowed(word) {
                    error_message = Some("Message has a hard word");
                    break;
                }
            }
        }
        if let Some(word_start) = current_word_start {
            let word = &message.content[word_start..];
            if !is_word_allowed(word) {
                error_message = Some("Message has a hard word");
            }
        }
    }
    if let Some(error_message) = error_message {
        message.delete(&ctx).await?;
        let dm_channel = message.author.create_dm_channel(&ctx).await?;
        dm_channel
            .send_message(&ctx, |new_message| {
                new_message.content(format!("{}! Your original message was:", error_message))
            })
            .await?;
        if !message.content.is_empty() {
            dm_channel
                .send_message(ctx, |new_message| new_message.content(&message.content))
                .await?;
        }
    }
    Ok(())
}

fn is_word_allowed(word: &str) -> bool {
    TOP_10K_WORDS.contains(&word.to_ascii_lowercase()[..])
}
