use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let addresses = prologix_rs::discover(None).await?;

    println!("Found {} controller(s):", addresses.len());

    for address in addresses {
        println!("{}", address);
    }

    Ok(())
}
