use env_logger::Builder;
use log::LevelFilter;
use tokio_s7::bytes_to_hex;
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
        .with_rack_slot(0, 1)
        .with_port(102);
    let mut client = S7Client::new(config);
    client.connect().await.expect("连接失败");

    // client.write_float32("DB3.1", 133.45).await.unwrap();
    // let result = client.read_float32("DB3.1").await;
    // println!("{:#?}", result);
    //
    // client.write_string("DB1.0", 100, "hello, this is a test data").await;
    // let result = client.read_string("DB1.0", 100).await;
    // println!("{:#?}", result);
    //
    // client.write_wstring("DB2.STRING20", 50, "你好啊朋友").await;
    // let result = client.read_wstring("DB2.STRING20", 50).await;
    // println!("{:#?}", result);

    // client.write_bool("DB2.X32.2", true).await;
    // let result = client.read_bool("DB2.X32.2").await;
    // println!("{:#?}", result);


    let result =  client.get_order_code().await.unwrap();
    println!("szl_read order_code: {:?}", result);

    let result =  client.get_cpu_info().await.unwrap();
    println!("szl_read cpu_info: {:?}", result);

    let result =  client.get_communication_info().await.unwrap();
    println!("szl_read communication_info: {:?}", result);

    // client.write_int16("DB2.W282", 1345).await;
    // let result = client.read_int16("DB2.W282").await;
    // println!("{:#?}", result);
    //
    // client.write_float32("DB2.REAL224", 1345.34).await;
    // let result = client.read_float32("DB2.REAL224").await;
    // println!("{:#?}", result);

    ()
}
