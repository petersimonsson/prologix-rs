use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let addresses = prologix_rs::discover(None).await?;

    println!("Addresses: {:?}", addresses);

    Ok(())
}
