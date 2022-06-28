use crate::{config, discord_bot};
use serde::Deserialize;
use std::borrow::{Borrow, Cow};

pub(crate) async fn handle_application(
    application_json: &str,
    discord_handle: &discord_bot::Handle,
) -> Result<(), crate::Error> {
    let app: Application = serde_json::from_str(application_json)?;
    config::get()
        .application_channel
        .send_message(discord_handle, move |message| {
            let embeds = ApplicationEmbeds::create(app);
            for embed in embeds.embeds {
                let url = embeds.url.clone();
                message.embed(move |discord_embed| {
                    discord_embed
                        .author(move |author| author.name(embed.author))
                        .title(embed.title)
                        .description(embed.description)
                        .fields(
                            embed
                                .fields
                                .into_iter()
                                .map(|field| (field.title, field.value, false)),
                        );
                    if url.len() <= EMBED_URL_LIMIT {
                        discord_embed.url(url)
                    }
                    discord_embed
                });
            }
            message
        })
        .await?;
    Ok(())
}

const DISCORD_QUESTION: usize = 0;
const IGN_QUESTION: usize = 1;

const EMBED_COUNT_LIMIT: usize = 10;
const EMBED_URL_LIMIT: usize = 2048;
const EMBED_TITLE_LIMIT: usize = 256;
const EMBED_AUTHOR_LIMIT: usize = 256;
const EMBED_FIELD_LIMIT: usize = 25;
const EMBED_FIELD_NAME_LIMIT: usize = 256;
const EMBED_FIELD_VALUE_LIMIT: usize = 1024;
const EMBED_CHARACTER_LIMIT: usize = 6000;

struct ApplicationEmbeds<'a> {
    url: String,
    embeds: Vec<ApplicationEmbed<'a>>,
}

impl<'a> ApplicationEmbeds<'a> {
    fn create(app: Application) -> Self {
        let (actual_questions, meta_questions): (Vec<_>, Vec<_>) = app
            .items
            .into_iter()
            .enumerate()
            .partition(|(index, _)| *index != DISCORD_QUESTION && *index != IGN_QUESTION);
        let mut discord_question = None;
        let mut ign_question = None;
        for (index, question) in meta_questions {
            if index == DISCORD_QUESTION {
                discord_question = Some(question);
            } else {
                ign_question = Some(question);
            }
        }

        Self {
            url: app.url,
            embeds: vec![ApplicationEmbed {
                title: discord_question
                    .map(|discord_name| {
                        trim(discord_name.answer.to_str(), EMBED_TITLE_LIMIT)
                            .into_owned()
                            .into()
                    })
                    .unwrap_or_else(|| "Submission".into()),
                author: ign_question
                    .map(|ign| {
                        trim(ign.answer.to_str(), EMBED_AUTHOR_LIMIT)
                            .into_owned()
                            .into()
                    })
                    .unwrap_or_else(|| "unknown".into()),
                description: "New submission from application form".into(),
                fields: actual_questions
                    .into_iter()
                    .map(|(_, item)| {
                        let answer = item.answer.to_str();
                        ApplicationField {
                            title: trim(item.question.into(), EMBED_FIELD_NAME_LIMIT)
                                .into_owned()
                                .into(),
                            value: if answer.is_empty() {
                                "[Empty]".into()
                            } else {
                                answer.into_owned().into()
                            },
                        }
                    })
                    .collect(),
            }],
        }
        .make_valid()
    }

    fn make_valid(mut self) -> Self {
        assert_eq!(self.embeds.len(), 1);
        let embed = self.embeds.remove(0);

        let mut fields: Vec<_> = embed
            .fields
            .into_iter()
            .flat_map(|field| {
                let value: Vec<_> = match field.value {
                    Cow::Owned(owned) => split_field(&owned)
                        .into_iter()
                        .map(|str| Cow::Owned(str.to_owned()))
                        .collect(),
                    Cow::Borrowed(borrowed) => split_field(borrowed)
                        .into_iter()
                        .map(Cow::Borrowed)
                        .collect(),
                };
                value
                    .into_iter()
                    .enumerate()
                    .map(move |(index, value)| ApplicationField {
                        title: if index == 0 {
                            field.title.clone()
                        } else {
                            trim(
                                format!("(cont.) {}", field.title).into(),
                                EMBED_FIELD_NAME_LIMIT,
                            )
                        },
                        value,
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        fields.reverse();

        let mut embeds = Vec::new();
        let mut current_embed = ApplicationEmbed {
            fields: Vec::new(),
            ..embed
        };
        let url_len = if self.url.len() > EMBED_URL_LIMIT {
            0
        } else {
            self.url.len()
        };
        while let Some(field) = fields.pop() {
            current_embed.fields.push(field);
            if current_embed.char_count() + url_len > EMBED_CHARACTER_LIMIT
                || current_embed.fields.len() > EMBED_FIELD_LIMIT
            {
                fields.push(current_embed.fields.pop().unwrap());
                embeds.push(current_embed);
                current_embed = ApplicationEmbed {
                    title: trim(
                        format!("(cont.) {}", embeds[0].title).into(),
                        EMBED_TITLE_LIMIT,
                    ),
                    author: embeds[0].author.clone(),
                    description: embeds[0].description.clone(),
                    fields: Vec::new(),
                };
            }
        }
        if !current_embed.fields.is_empty() {
            embeds.push(current_embed);
        }

        while embeds.len() > EMBED_COUNT_LIMIT {
            embeds.pop().unwrap();
        }

        ApplicationEmbeds { embeds, ..self }
    }
}

fn split_field<T>(value: &T) -> Vec<&str>
where
    T: Borrow<str> + ?Sized,
{
    let value = value.borrow();
    if value.len() <= EMBED_FIELD_VALUE_LIMIT {
        return vec![value];
    }
    let mut result = Vec::new();
    let mut last_split = 0;
    let mut last_word_boundary = 0;
    let mut last_next_word_boundary = 0;
    for (index, char) in value.char_indices() {
        if index - last_split > EMBED_FIELD_VALUE_LIMIT {
            if last_word_boundary != last_split {
                result.push(&value[last_split..last_word_boundary]);
                last_word_boundary = last_next_word_boundary;
                last_split = last_word_boundary;
            } else {
                last_word_boundary = index;
                last_next_word_boundary = index;
                result.push(&value[last_split..last_word_boundary]);
                last_split = index;
            }
        }
        if char.is_whitespace() {
            last_word_boundary = index;
            last_next_word_boundary = index + char.len_utf8();
        }
    }
    result.push(&value[last_split..]);
    result
}

struct ApplicationEmbed<'a> {
    title: Cow<'a, str>,
    author: Cow<'a, str>,
    description: Cow<'a, str>,
    fields: Vec<ApplicationField<'a>>,
}

impl<'a> ApplicationEmbed<'a> {
    fn char_count(&self) -> usize {
        self.title.len()
            + self.author.len()
            + self.description.len()
            + self
                .fields
                .iter()
                .map(|field| field.char_count())
                .sum::<usize>()
    }
}

struct ApplicationField<'a> {
    title: Cow<'a, str>,
    value: Cow<'a, str>,
}

impl<'a> ApplicationField<'a> {
    fn char_count(&self) -> usize {
        self.title.len() + self.value.len()
    }
}

fn trim(str: Cow<str>, max_len: usize) -> Cow<str> {
    if str.len() > max_len {
        format!(
            "{}…",
            &str[..str
                .char_indices()
                .map(|(index, _)| index)
                .take_while(|&index| index <= max_len - "…".len())
                .last()
                .expect("max_len too small")]
        )
        .into()
    } else {
        str
    }
}

#[derive(Deserialize)]
struct Application {
    url: String,
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    question: String,
    answer: Answer,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Answer {
    String(String),
    StringArray(Vec<String>),
    StringArray2d(Vec<Vec<String>>),
}

impl Answer {
    fn to_str(&self) -> Cow<str> {
        match self {
            Answer::String(str) => Cow::Borrowed(&*str),
            Answer::StringArray(strs) => strs.join("\r\n").into(),
            Answer::StringArray2d(strs) => strs
                .iter()
                .map(|strs| strs.join(", "))
                .collect::<Vec<_>>()
                .join("\r\n")
                .into(),
        }
    }
}
