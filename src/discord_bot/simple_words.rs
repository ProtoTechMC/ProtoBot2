use crate::config;
use lazy_static::lazy_static;
use serenity::client::Context;
use serenity::model::id::{ChannelId, MessageId};
use serenity::model::user::User;
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
        error_message = Some("Message has an image".to_owned());
    } else {
        let mut illegal_words = Vec::new();
        let mut current_word_start = None;
        let mut prev_char = None;
        for (index, char) in content.char_indices() {
            if !char.is_ascii() {
                error_message = Some("Message has a hard character".to_owned());
                break;
            }
            if is_allowed_in_word(content, index, char, prev_char) {
                if current_word_start.is_none() {
                    current_word_start = Some(index);
                }
            } else if let Some(word_start) = current_word_start {
                current_word_start = None;
                let word = &content[word_start..index];
                if !is_word_allowed(word) {
                    illegal_words.push(word.to_ascii_lowercase());
                }
            }
            prev_char = Some(char);
        }
        if let Some(word_start) = current_word_start {
            let word = &content[word_start..];
            if !is_word_allowed(word) {
                illegal_words.push(word.to_ascii_lowercase());
            }
        }
        if error_message.is_none() && !illegal_words.is_empty() {
            illegal_words.dedup();
            error_message = Some(format!(
                "Message has the following hard words: {}",
                illegal_words
                    .into_iter()
                    .take(5)
                    .map(|word| format!("\"{}\"", word))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if let Some(error_message) = error_message {
        channel_id.delete_message(&ctx, message_id).await?;
        let dm_channel = author.create_dm_channel(&ctx).await?;
        dm_channel
            .send_message(&ctx, |new_message| {
                new_message.content(format!("{}! Your original message was:", error_message))
            })
            .await?;
        if !content.is_empty() {
            dm_channel
                .send_message(ctx, |new_message| new_message.content(content))
                .await?;
        }
    }
    Ok(())
}

fn is_allowed_in_word(whole: &str, index: usize, char: char, prev_char: Option<char>) -> bool {
    if char.is_ascii_alphabetic() {
        return true;
    }
    if char == '\'' {
        if let Some(prev_char) = prev_char {
            if prev_char.is_ascii_alphabetic() {
                let next_index = index + char.len_utf8();
                if next_index < whole.len() {
                    let next_char = whole[next_index..].chars().next().unwrap();
                    if next_char.is_ascii_alphabetic() {
                        return true;
                    }
                }
            }
        }
    }

    false
}

fn is_word_allowed(word: &str) -> bool {
    let lowercase = word.to_ascii_lowercase().replace('\'', "");
    TOP_10K_WORDS.contains(&lowercase[..])
        || (lowercase.ends_with('s') && TOP_10K_WORDS.contains(&lowercase[..lowercase.len() - 1]))
}
