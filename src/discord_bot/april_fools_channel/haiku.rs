use crate::discord_bot::april_fools_channel::AprilFoolsChannel;
use arpabet::phoneme::Phoneme;
use arpabet::{load_cmudict, Polyphone};
use std::collections::BTreeSet;
use std::mem;

const INVALID_HAIKU: &str = "Not quite a haiku
Your rhythm is imperfect
Try once more, my friend";

const INVALID_CHARACTER: &str = "Letters plain and clear,
no strange symbols may appear.
ASCII rules this place.";

const NO_DIGITS: &str = "Numbers in your text,
A word is what we seek here,
Please try once again.";

const NO_ATTACHMENTS: &str = "No files, no uploads,
only words may travel here.
Speak in text alone.";

pub(crate) struct HaikuChannel;

pub(crate) static CHANNEL: HaikuChannel = HaikuChannel;

impl AprilFoolsChannel for HaikuChannel {
    fn get_error(&self, message: &str) -> Option<String> {
        if !message.is_ascii() {
            return Some(INVALID_CHARACTER.to_owned());
        }
        if message.contains(|char: char| char.is_ascii_digit()) {
            return Some(NO_DIGITS.to_owned());
        }

        let lines: Vec<_> = message.lines().collect();
        if lines.len() != 3 {
            return Some(INVALID_HAIKU.to_owned());
        }

        if message_seems_cheaty(message) {
            return Some(INVALID_HAIKU.to_owned());
        }

        let syllable_count_1 = get_line_syllables(lines[0]);
        let syllable_count_2 = get_line_syllables(lines[1]);
        let syllable_count_3 = get_line_syllables(lines[2]);

        if !syllable_count_1.contains(&5)
            || !syllable_count_2.contains(&7)
            || !syllable_count_3.contains(&5)
        {
            return Some(format!(
                "{}\nSyllable counts are: {}, {}, {}",
                INVALID_HAIKU,
                display_syllable_count(&syllable_count_1),
                display_syllable_count(&syllable_count_2),
                display_syllable_count(&syllable_count_3)
            ));
        }

        None
    }

    fn your_original_message_was(&self) -> Option<&'static str> {
        None
    }

    fn has_attachment_message(&self) -> &'static str {
        NO_ATTACHMENTS
    }
}

fn message_seems_cheaty(message: &str) -> bool {
    let mut punctuation_count = 0;
    for c in message.bytes() {
        if !c.is_ascii_whitespace() && !c.is_ascii_alphabetic() {
            punctuation_count += 1;
        }
    }

    let ratio = punctuation_count as f32 / message.len() as f32;
    ratio > 0.3
}

fn get_line_syllables(line: &str) -> BTreeSet<u32> {
    let mut result: BTreeSet<u32> = [0].into_iter().collect();
    let mut next_result = BTreeSet::new();

    for word in line.split(|char: char| !char.is_ascii_alphabetic() && char != '\'') {
        let word = word
            .trim_start_matches('\'')
            .trim_end_matches('\'')
            .to_ascii_lowercase();
        if word.is_empty() {
            continue;
        }

        let cmudict = load_cmudict(); // this is lazy static
        if let Some(polyphone) = cmudict.get_polyphone_ref(&word) {
            let syllables = check_word_cmu(polyphone);
            for &prev_syllables in &result {
                next_result.insert(prev_syllables + syllables);
            }
            let mut alt_index = 1u32;
            loop {
                let alt_word = format!("{}({})", word, alt_index);
                let Some(polyphone) = cmudict.get_polyphone_ref(&alt_word) else {
                    break;
                };
                let syllables = check_word_cmu(polyphone);
                for &prev_syllables in &result {
                    next_result.insert(prev_syllables + syllables);
                }
                alt_index += 1;
            }
        } else {
            let syllables = check_word_simple(&word);
            for &prev_syllables in &result {
                next_result.insert(syllables + prev_syllables);
            }
        }

        mem::swap(&mut result, &mut next_result);
        next_result.clear();
    }

    result
}

fn check_word_cmu(polyphone: &Polyphone) -> u32 {
    1.max(
        polyphone
            .iter()
            .filter(|phoneme| matches!(phoneme, Phoneme::Vowel(_)))
            .count() as u32,
    )
}

fn check_word_simple(word: &str) -> u32 {
    let word = word.as_bytes();
    let mut syllables = 0;

    for i in 0..word.len() {
        let char = word[i];
        let prev_char = if i == 0 { b'_' } else { word[i - 1] };

        if is_vowel(char) && !is_vowel(prev_char) && (char != b'e' || i != word.len() - 1) {
            syllables += 1;
        }
    }

    1.max(syllables)
}

fn is_vowel(c: u8) -> bool {
    matches!(c, b'a' | b'e' | b'i' | b'o' | b'u' | b'y')
}

fn display_syllable_count(syllables: &BTreeSet<u32>) -> String {
    if syllables.len() == 1 {
        syllables.first().unwrap().to_string()
    } else {
        let mut result = "(".to_owned();
        for (i, s) in syllables.iter().enumerate() {
            if i > 0 {
                if i == syllables.len() - 1 {
                    result.push_str(" or ");
                } else {
                    result.push_str(", ");
                }
            }
            result.push_str(&s.to_string());
        }
        result.push(')');
        result
    }
}
