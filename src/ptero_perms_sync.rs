use crate::ProtobotData;
use log::{error, info};

pub(crate) async fn run(
    data: &ProtobotData,
    mut args: impl Iterator<Item = &str>,
) -> Result<(), crate::Error> {
    let Some(server) = args.next() else {
        error!("Missing server argument");
        return Ok(());
    };

    let server = data.pterodactyl.get_server(server);
    let existing_users = server.list_users().await?;
    info!("{:#?}", existing_users);

    Ok(())
}
