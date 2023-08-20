use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    prologix_rs::reboot(&args[1].parse()?, &prologix_rs::RebootType::Reset).await?;

    Ok(())
}
