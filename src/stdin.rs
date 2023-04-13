use crate::ProtobotData;
use crate::{config, ptero_perms_sync};
use log::{error, info};
use tokio::io::AsyncBufReadExt;

pub(crate) async fn handle_stdin_loop(data: ProtobotData) -> Result<(), crate::Error> {
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = stdin.next_line().await? {
        if let Err(err) = handle_command(&data, line.split(' ')).await {
            error!("Error processing console command: {}", err);
        }
    }
    Ok(())
}

macro_rules! declare_commands {
    ($(($name:literal, $func:path, $description:literal);)*) => {
        async fn handle_command(data: &ProtobotData, mut args: impl Iterator<Item = &str>) -> Result<(), crate::Error> {
            let Some(command) = args.next() else { return Ok(()); };
            match command {
                $(
                $name => $func(data, args).await,
                )*
                _ => {
                    show_help();
                    Ok(())
                },
            }
        }

        fn show_help() {
            info!("ProtoBot console help");
            $(
            info!(concat!($name, ": ", $description));
            )*
            info!("help: displays this message.");
        }
    }
}

declare_commands! {
    ("perms_sync", ptero_perms_sync::run, "synchronizes user permissions on a ptero server");
    ("reload", reload_config, "reloads bot config");
}

async fn reload_config(
    _data: &ProtobotData,
    _args: impl Iterator<Item = &str>,
) -> Result<(), crate::Error> {
    config::reload()
}
