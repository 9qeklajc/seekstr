use std::time::Duration;

use nostr_sdk::prelude::*;

pub async fn collect() -> Result<()> {
    tracing_subscriber::fmt::init();

    let _public_key =
        PublicKey::from_bech32("npub1080l37pfvdpyuzasyuy2ytjykjvq3ylr5jlqlg7tvzjrh9r8vn3sf5yaph")?;

    let keys = Keys::parse("nsec1ufnus6pju578ste3v90xd5m2decpuzpql2295m3sknqcjzyys9ls0qlc85")?;
    let client = Client::new(keys);

    client.add_relay("ws://localhost:8080").await?;

    client.connect().await;

    // Publish a text note
    let builder = EventBuilder::text_note("Hello world");
    let output = client.send_event_builder(builder).await?;
    println!("Event ID: {}", output.id().to_bech32()?);
    println!("Sent to: {:?}", output.success);
    println!("Not sent to: {:?}", output.failed);

    // Create a text note POW event to relays
    let builder = EventBuilder::text_note("POW text note from rust-nostr").pow(20);
    client.send_event_builder(builder).await?;

    // Send a text note POW event to specific relays
    let builder = EventBuilder::text_note("POW text note from rust-nostr 16").pow(16);
    client
        .send_event_builder_to(["ws://localhost:8080"], builder)
        .await?;

    let client = Client::default();
    client.add_relay("ws://localhost:8080").await?;

    client.connect().await;

    // let filter = Filter::new().author(public_key).kind(Kind::Metadata);
    // let events = client.fetch_events(filter, Duration::from_secs(10)).await?;
    // println!("{events:#?}");

    let filter = Filter::new().limit(3);
    let events = client
        .fetch_events_from(["ws://localhost:8080"], filter, Duration::from_secs(10))
        .await?;
    println!("{events:#?}");

    Ok(())
}
