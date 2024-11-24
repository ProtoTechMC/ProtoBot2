use crate::{config, smp_commands};
use pterodactyl_api::client::backups::Backup;
use pterodactyl_api::client::websocket::{PteroWebSocketHandle, PteroWebSocketListener};
use pterodactyl_api::client::{PowerSignal, ServerState};
use serenity::builder::EditInteractionResponse;
use serenity::client::Context;
use serenity::model::application::CommandInteraction;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Mutex;

static COPY_UPDATE_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
fn copy_update_mutex() -> &'static Mutex<()> {
    COPY_UPDATE_MUTEX.get_or_init(|| Mutex::new(()))
}

pub(crate) async fn run(
    ctx: &Context,
    command: &CommandInteraction,
    pterodactyl: &pterodactyl_api::client::Client,
) -> Result<(), crate::Error> {
    let _guard = match copy_update_mutex().try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            command
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new()
                        .content("Copy is already currently being updated"),
                )
                .await?;
            return Ok(());
        }
    };

    let config = config::get();

    let smp_server = pterodactyl.get_server(&config.pterodactyl_smp);
    let copy_server = pterodactyl.get_server(&config.pterodactyl_smp_copy);

    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content("Updating copy...\nStopping SMP"),
        )
        .await?;
    stop_and_wait(&smp_server).await?;
    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content("Updating copy...\nStopping copy"),
        )
        .await?;
    stop_and_wait(&copy_server).await?;

    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new()
                .content("Updating copy...\nCreating SMP backup to copy from"),
        )
        .await?;
    let backup = create_backup_and_wait(&smp_server).await?;

    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new()
                .content("Updating copy...\nCreating pre-overwrite copy backup"),
        )
        .await?;
    create_backup_and_wait(&copy_server).await?;

    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content(
                "Updating copy...\nCopying backup from SMP to copy. This may take a while",
            ),
        )
        .await?;

    copy_server.delete_file("copytemp").await?;
    copy_server.create_folder("copytemp").await?;

    let backup_download = smp_server.get_backup_download_link(backup.uuid).await?;
    copy_server
        .write_file(
            "copytemp/backup.tar.gz",
            reqwest::get(backup_download).await?,
        )
        .await?;

    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content("Updating copy...\nExtracting backup"),
        )
        .await?;
    copy_server.create_folder("copytemp/backup").await?;
    copy_server
        .decompress_file("copytemp/backup.tar.gz", "copytemp/backup")
        .await?;

    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content("Updating copy...\nCopying world"),
        )
        .await?;
    copy_server.delete_file("world").await?;
    copy_server
        .rename_file("copytemp/backup/world", "world")
        .await?;

    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content("Updating copy...\nCleaning up"),
        )
        .await?;
    copy_server.delete_file("copytemp").await?;

    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content("Updating copy...\nStarting servers"),
        )
        .await?;
    smp_server.send_power_signal(PowerSignal::Start).await?;
    copy_server.send_power_signal(PowerSignal::Start).await?;

    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content("Copy has been updated"),
        )
        .await?;

    Ok(())
}

async fn stop_and_wait(server: &pterodactyl_api::client::Server<'_>) -> Result<(), crate::Error> {
    let current_state = server.get_resources().await?.current_state;
    if current_state == ServerState::Stopping || current_state == ServerState::Offline {
        return Ok(());
    }
    struct Listener;
    impl<H: PteroWebSocketHandle> PteroWebSocketListener<H> for Listener {
        async fn on_ready(&mut self, handle: &mut H) -> pterodactyl_api::Result<()> {
            handle.send_power_signal(PowerSignal::Stop).await
        }

        async fn on_status(
            &mut self,
            handle: &mut H,
            status: ServerState,
        ) -> pterodactyl_api::Result<()> {
            if status == ServerState::Offline {
                handle.disconnect();
            }
            Ok(())
        }
    }
    server
        .run_websocket_loop(
            |url| async { Ok(async_tungstenite::tokio::connect_async(url).await?.0) },
            Listener,
        )
        .await?;
    Ok(())
}

async fn create_backup_and_wait(
    server: &pterodactyl_api::client::Server<'_>,
) -> Result<Backup, crate::Error> {
    let mut backup = smp_commands::create_backup(
        server,
        Some(format!("Pre copy update {}", chrono::Utc::now())),
    )
    .await?;
    while backup.completed_at.is_none() {
        backup = server.get_backup(backup.uuid).await?;
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    Ok(backup)
}
