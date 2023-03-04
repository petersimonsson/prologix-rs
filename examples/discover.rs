use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let addresses = prologix_rs::discover().await?;

    println!("Addresses: {:?}", addresses);

    Ok(())
}
