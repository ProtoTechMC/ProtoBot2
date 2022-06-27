use crate::{config, discord_bot};
use serde::Deserialize;
use serenity::model::Timestamp;

pub(crate) async fn handle_application(
    application_json: &str,
    discord_handle: &discord_bot::Handle,
) -> Result<(), crate::Error> {
    let app: Application = serde_json::from_str(application_json)?;
    config::get()
        .application_channel
        .send_message(discord_handle, move |message| {
            message.add_embed(move |embed| {
                embed
                    .title("Submission")
                    .description("New submission from application form")
                    .url(app.url)
                    .timestamp(Timestamp::now())
                    .fields(app.items.iter().map(|item| {
                        let mut answer_str = item.answer.to_str();
                        if answer_str.is_empty() {
                            answer_str = "[Empty]".to_owned();
                        }
                        (trim(item.question.to_owned(), 256), answer_str, false)
                    }))
            })
        })
        .await?;
    Ok(())
}

fn trim(str: String, max_len: usize) -> String {
    if str.len() > max_len {
        format!("{}…", &str[..max_len - 1])
    } else {
        str
    }
}

#[derive(Deserialize)]
struct Application<'a> {
    url: &'a str,
    items: Vec<Item<'a>>,
}

#[derive(Deserialize)]
struct Item<'a> {
    question: &'a str,
    answer: Answer<'a>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Answer<'a> {
    String(&'a str),
    StringArray(Vec<&'a str>),
    StringArray2d(Vec<Vec<&'a str>>),
}

impl<'a> Answer<'a> {
    fn to_str(&self) -> String {
        match self {
            Answer::String(str) => String::from(*str),
            Answer::StringArray(strs) => strs.join("\r\n"),
            Answer::StringArray2d(strs) => strs
                .iter()
                .map(|strs| strs.join(", "))
                .collect::<Vec<_>>()
                .join("\r\n"),
        }
    }
}
