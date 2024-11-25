use crate::ProtobotData;
use crate::{config, ptero_perms_sync};
use log::{error, info};
use std::io;
use std::io::BufRead;

pub(crate) fn handle_stdin_loop(runtime: &tokio::runtime::Runtime, data: ProtobotData) {
    let lines = io::BufReader::new(io::stdin()).lines();
    for line in lines {
        match line {
            Ok(line) => {
                if !crate::is_shutdown() {
                    let data = data.clone();
                    runtime.spawn(async move {
                        if let Err(err) = handle_command(&data, line.split(' ')).await {
                            error!("Error while handling stdin: {}", err);
                        }
                    });
                }
            }
            Err(err) => {
                error!("Failed to read line: {}", err);
            }
        }
    }
}

macro_rules! declare_commands {
    ($(($name:literal, $func:path, $description:literal);)*) => {
        pub async fn handle_command(data: &ProtobotData, mut args: impl Iterator<Item = &str>) -> Result<(), crate::Error> {
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
    ("stop", stop, "stops the bot");
}

async fn reload_config(
    _data: &ProtobotData,
    _args: impl Iterator<Item = &str>,
) -> Result<(), crate::Error> {
    config::reload()?;
    info!("Reloaded config");
    Ok(())
}

async fn stop(_data: &ProtobotData, _args: impl Iterator<Item = &str>) -> Result<(), crate::Error> {
    crate::shutdown();
    Ok(())
}
