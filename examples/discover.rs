use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let addresses = prologix_rs::discover(None).await?;

    match addresses {
        Some(addresses) => {
            println!("Found {} controller(s):", addresses.len());
            for address in addresses {
                println!("{}", address);
            }
        }
        None => println!("Found no controllers"),
    }

    Ok(())
}
