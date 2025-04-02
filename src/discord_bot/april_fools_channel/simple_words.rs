use crate::discord_bot::april_fools_channel::AprilFoolsChannel;
use std::collections::HashSet;
use std::sync::OnceLock;

fn top_10k_words() -> &'static HashSet<&'static str> {
    static TOP_10K_WORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    TOP_10K_WORDS.get_or_init(|| {
        let mut set = HashSet::with_capacity(5000);
        for word in include!("../simple_writer_words.txt") {
            set.insert(word);
        }
        set
    })
}

pub(crate) struct SimpleWordsChannel;

pub(crate) static CHANNEL: SimpleWordsChannel = SimpleWordsChannel;

impl AprilFoolsChannel for SimpleWordsChannel {
    fn get_error(&self, message: &str) -> Option<String> {
        let mut illegal_words = Vec::new();
        let mut current_word_start = None;
        let mut prev_char = None;
        for (index, char) in message.char_indices() {
            if !char.is_ascii() && char != '’' {
                return Some("Message has a hard letter!".to_owned());
            }
            if is_allowed_in_word(message, index, char, prev_char) {
                if current_word_start.is_none() {
                    current_word_start = Some(index);
                }
            } else if let Some(word_start) = current_word_start {
                current_word_start = None;
                let word = &message[word_start..index];
                if !is_word_allowed(word) {
                    illegal_words.push(word.to_ascii_lowercase());
                }
            }
            prev_char = Some(char);
        }
        if let Some(word_start) = current_word_start {
            let word = &message[word_start..];
            if !is_word_allowed(word) {
                illegal_words.push(word.to_ascii_lowercase());
            }
        }
        if !illegal_words.is_empty() {
            illegal_words.dedup();
            return Some(format!(
                "Message has the following hard words: {}!",
                illegal_words
                    .into_iter()
                    .take(5)
                    .map(|word| format!("\"{}\"", word))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        None
    }

    fn has_attachment_message(&self) -> &'static str {
        "Message has a picture!"
    }
}

fn is_allowed_in_word(whole: &str, index: usize, char: char, prev_char: Option<char>) -> bool {
    if char.is_ascii_alphabetic() {
        return true;
    }
    if char == '\'' || char == '’' {
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
    let lowercase = word.to_ascii_lowercase();
    top_10k_words().contains(&lowercase[..])
        || (lowercase.ends_with('s') && top_10k_words().contains(&lowercase[..lowercase.len() - 1]))
}
