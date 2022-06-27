use log::info;
use serde::Deserialize;

pub(crate) async fn handle_application(application_json: &str) -> Result<(), crate::Error> {
    let app: Application = serde_json::from_str(application_json)?;
    for (index, item) in app.items.iter().enumerate() {
        info!("Question {}", index + 1);
        info!("{}", item.question);
        info!("> {:?}", item.answer);
    }
    Ok(())
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
