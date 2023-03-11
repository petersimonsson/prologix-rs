use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let controllers = prologix_rs::discover(None).await?;

    println!("Found {} controller(s):", controllers.len());

    for controller in controllers {
        println!("{} - {}", controller.ip_addr(), controller.mac_addr());
    }

    Ok(())
}
