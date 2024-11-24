use crate::{config, ProtobotData};
use linkify::{LinkFinder, LinkKind};
use log::warn;
use scraper::Html;
use serde::Deserialize;
use serenity::builder::{CreateEmbed, CreateEmbedAuthor, CreateMessage};
use std::borrow::{Borrow, Cow};
use std::collections::HashMap;

pub(crate) async fn handle_application(
    application_json: &str,
    data: &ProtobotData,
) -> Result<(), crate::Error> {
    let app: Application = serde_json::from_str(application_json)?;
    let attachments = find_attachments(&app).await;
    let embeds = ApplicationEmbeds::create(app);

    for embed in embeds.embeds {
        let url = embeds.url.clone();
        config::get()
            .application_channel
            .send_message(
                &data.discord_handle,
                CreateMessage::new().embed({
                    let mut discord_embed = CreateEmbed::new()
                        .author(CreateEmbedAuthor::new(embed.author))
                        .title(embed.title)
                        .description(embed.description)
                        .fields(
                            embed
                                .fields
                                .into_iter()
                                .map(|field| (field.title, field.value, false)),
                        );
                    if url.len() <= EMBED_URL_LIMIT {
                        discord_embed = discord_embed.url(url);
                    }
                    discord_embed
                }),
            )
            .await?;
    }

    for attachment in attachments {
        config::get()
            .application_channel
            .send_message(
                &data.discord_handle,
                CreateMessage::new().embed({
                    let mut discord_embed = CreateEmbed::new();
                    match attachment.typ {
                        AttachmentType::Image => {
                            discord_embed = discord_embed.image(attachment.url);
                        }
                        AttachmentType::Video => {
                            // discord can't attach videos: https://github.com/serenity-rs/serenity/issues/2354
                        }
                    }
                    if let Some(link) = attachment.link {
                        discord_embed = discord_embed.url(link);
                    }
                    if let Some(title) = attachment.title {
                        discord_embed = discord_embed.title(title);
                    }
                    if let Some(description) = attachment.description {
                        discord_embed = discord_embed.description(description);
                    }
                    discord_embed
                }),
            )
            .await?;
    }

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
            if current_embed.fields.len() > 1
                && (current_embed.char_count() + url_len > EMBED_CHARACTER_LIMIT
                    || current_embed.fields.len() > EMBED_FIELD_LIMIT)
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

struct Attachment {
    typ: AttachmentType,
    url: String,
    link: Option<String>,
    title: Option<String>,
    description: Option<String>,
}

enum AttachmentType {
    Image,
    Video,
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

fn find_urls(str: &str) -> impl Iterator<Item = &str> {
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]);
    finder
        .links(str)
        .map(|link| link.as_str())
        .filter(|link| link.starts_with("https://"))
}

async fn find_attachments(app: &Application) -> Vec<Attachment> {
    let mut result = Vec::new();

    let client = reqwest::Client::new();

    let urls = app
        .items
        .iter()
        .flat_map(|item| match &item.answer {
            Answer::String(str) => Some(str),
            _ => None,
        })
        .flat_map(|answer| find_urls(answer))
        .collect::<Vec<_>>();
    for url in urls {
        let response = match client.get(url).send().await {
            Ok(res) => res,
            Err(err) => {
                warn!("Failed to fetch from specified url {}: {}", url, err);
                continue;
            }
        };
        if !response.status().is_success() {
            warn!(
                "Failed to fetch from specified url {}: returned status code {}",
                url,
                response.status()
            );
            continue;
        }
        let content_type = match response
            .headers()
            .get("Content-Type")
            .and_then(|header| header.to_str().ok())
        {
            Some(header) => header,
            None => {
                warn!("Specified url {} missing content type header", url);
                continue;
            }
        };

        if content_type.starts_with("image/") {
            result.push(Attachment {
                typ: AttachmentType::Image,
                url: url.to_owned(),
                link: None,
                title: None,
                description: None,
            });
        } else if content_type.starts_with("video/") {
            result.push(Attachment {
                typ: AttachmentType::Video,
                url: url.to_owned(),
                link: None,
                title: None,
                description: None,
            });
        } else if content_type.starts_with("text/html") {
            let text = match response.text().await {
                Ok(text) => text,
                Err(err) => {
                    warn!("Failed to download from url {}: {}", url, err);
                    continue;
                }
            };
            let document = Html::parse_document(&text);
            let tags: HashMap<_, _> = document
                .root_element()
                .children()
                .find(|child| {
                    child
                        .value()
                        .as_element()
                        .map(|element| element.name() == "head")
                        == Some(true)
                })
                .map(|head| {
                    head.children()
                        .flat_map(|child| child.value().as_element())
                        .filter(|&element| element.name() == "meta")
                        .flat_map(|element| element.attr("property").zip(element.attr("content")))
                })
                .into_iter()
                .flatten()
                .collect();

            if tags.get("og:site_name") == Some(&"YouTube")
                && tags.get("og:type") == Some(&"video.other")
            {
                result.push(Attachment {
                    typ: AttachmentType::Video,
                    url: tags.get("og:video:url").copied().unwrap_or(url).to_owned(),
                    link: tags.get("og:url").copied().map(|str| str.to_owned()),
                    title: tags.get("og:title").copied().map(|str| str.to_owned()),
                    description: tags
                        .get("og:description")
                        .copied()
                        .map(|str| str.to_owned()),
                })
            } else if tags.contains_key("og:image")
                || (tags.contains_key("og:title") && tags.contains_key("og:url"))
            {
                result.push(Attachment {
                    typ: AttachmentType::Image,
                    url: tags
                        .get("og:image")
                        .copied()
                        .unwrap_or_else(|| tags["og:url"])
                        .to_owned(),
                    link: tags.get("og:url").copied().map(|str| str.to_owned()),
                    title: tags.get("og:title").copied().map(|str| str.to_owned()),
                    description: tags
                        .get("og:description")
                        .copied()
                        .map(|str| str.to_owned()),
                })
            }
        }
    }

    result
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
            Answer::String(str) => Cow::Borrowed(&**str),
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
