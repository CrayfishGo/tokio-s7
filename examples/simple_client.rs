use env_logger::Builder;
use log::LevelFilter;
use tokio_s7::client::{S7Client, S7Config};
use tokio_s7::types::PlcType;

#[tokio::main]
async fn main() {
    Builder::new()
        .filter(None, LevelFilter::Info) // 设置全局日志级别
        .format_timestamp_millis() // 可选: 添加时间戳
        .init();

    let config = S7Config::new("10.211.55.3")
        .with_plc_type(PlcType::S1200)
        .with_port(102);
    let mut client = S7Client::new(config);
    client.connect().await.expect("连接失败");

    client.write_float32("DB3.1", 133.45).await.unwrap();
    let result = client.read_float32("DB3.1").await;
    println!("{:#?}", result);

    client.write_string("DB1.0", 100, "hello, this is a test data").await;
    let result = client.read_string("DB1.0", 100).await;
    println!("{:#?}", result);

    client.write_wstring("DB2.0", 50, "你好啊朋友").await;
    let result = client.read_wstring("DB2.0", 50).await;
    println!("{:#?}", result);

    ()
}
