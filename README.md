# Loco Twitch (WIP)

Loco Twitch is a Synchronous IRC client with focus on Twitch IRC chat.


Usage:

```toml
[dependencies]
loco-twitch = "0.1.0"
```

```rust
fn main() {
    let loco_config = LocoConfig::new(oauth, nickname, channel_to_join);
    let mut loco_connection = LocoConnection::new(loco_config).unwrap();
    while let Some(irc) = loco_connection.next() {
        //do something with IRC
    }
}
```