use crate::config;
use async_trait::async_trait;
use log::{error, info};
use serenity::client::{Context, EventHandler};
use serenity::model::gateway::Ready;
use serenity::model::id::GuildId;
use serenity::model::interactions::application_command::ApplicationCommandInteraction;
use serenity::model::interactions::{Interaction, InteractionResponseType};
use serenity::prelude::GatewayIntents;
use serenity::Client;

struct Handler;

async fn create_commands(ctx: &Context, guild_id: GuildId) -> serenity::Result<()> {
    guild_id
        .set_application_commands(&ctx.http, |commands| {
            commands.create_application_command(|command| {
                command.name("hello").description("A test command")
            })
        })
        .await?;
    Ok(())
}

#[allow(clippy::single_match)]
async fn process_command(
    ctx: &Context,
    command: ApplicationCommandInteraction,
) -> serenity::Result<()> {
    match &command.data.name[..] {
        "hello" => {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| message.content("Hello World!"))
                })
                .await?;
        }
        _ => {}
    }
    Ok(())
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _data_about_bot: Ready) {
        if let Err(err) = create_commands(&ctx, config::get().guild_id).await {
            error!("Failed to register commands: {}", err);
            return;
        }

        info!("Discord bot ready");
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            if let Err(err) = process_command(&ctx, command).await {
                error!("Failed to process command: {}", err);
            }
        }
    }
}

pub(crate) async fn run() -> Result<(), crate::Error> {
    let intents = GatewayIntents::empty();
    let mut client = Client::builder(&config::get().discord_token, intents)
        .event_handler(Handler)
        .await?;
    client.start().await?;
    Ok(())
}
