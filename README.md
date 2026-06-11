# tokio-s7

Async Rust client library for Siemens S7 PLCs over ISO-on-TCP. Pure Rust — no FFI, no native C dependency.


## Features

| Capability                                                            | Status |
|-----------------------------------------------------------------------|---|
| **S7-300/400/200/1200/1500** — read/write DB, multi-area, blocks, SZL | ✅ |
| **Multi-read / multi-write** with automatic PDU batching              | ✅ |
| **S7Area read/write** with type decoding/encoding                     | ✅ |
| **PLC information** — order code, CPU info, CP info, module list      | ✅ |
| **SZL queries** — system status list, SZL directory                   | ✅ |
| **Reconnect** — re-establish TCP + S7 handshake in-place              | ✅ |
| **Pure Rust, zero native dependencies**                               | ✅ |

## Add to your project

```toml
[dependencies]
tokio-s7 = "0.1"
```

## Quick start

### Single connection

```rust
use env_logger::Builder;
use log::LevelFilter;
use std::time::Duration;
use tokio::signal;
use tokio::time::interval;
use tokio_s7::client::{S7Client, S7Config};
use tokio_s7::types::PlcType;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = S7Config::new("10.211.55.3")
        .with_plc_type(PlcType::S1200)
        .with_rack_slot(0, 1)
        .with_auto_reconnect(true)
        .with_port(102);
    let mut client = S7Client::new(config);
    client.connect().await.expect("connect error");

    client.write_int16("DB2.W282", 32).await;
    let result = client.read_int16("DB2.W282").await;
    println!("{:#?}", result);

    Ok(())
}
```

## License

Apache-2.0 — see [LICENSE](LICENSE).
