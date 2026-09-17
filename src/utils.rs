use std::io;
use std::path::Path;
use tokio::io::AsyncWriteExt;

pub async fn write_safe(
    file_name: impl AsRef<Path>,
    temp_file_name: impl AsRef<Path>,
    data: &[u8],
) -> io::Result<()> {
    let mut temp_file = tokio::fs::File::create(temp_file_name.as_ref()).await?;
    temp_file.write_all(data).await?;
    temp_file.sync_all().await?;
    tokio::fs::rename(temp_file_name, file_name).await?;
    Ok(())
}
