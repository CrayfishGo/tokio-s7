use crate::error::S7Error;
use crate::item::{DataItem, ReturnItem, S7ReturnItem};
use crate::types::{S7FunctionCode, S7MessageType, S7ReturnCode};
use tokio_util::bytes::{Buf, BufMut, BytesMut};

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct ReadWriteDatum {
    pub return_items: Vec<S7ReturnItem>,
}

impl ReadWriteDatum {
    pub fn new(data_items: Vec<DataItem>) -> ReadWriteDatum {
        Self {
            return_items: data_items
                .into_iter() // 消耗向量，转移所有权
                .map(S7ReturnItem::Data) // 无需闭包，直接传递枚举变体函数
                .collect(),
        }
    }

    pub fn add_item(&mut self, return_item: S7ReturnItem) {
        self.return_items.push(return_item);
    }
}

impl Default for ReadWriteDatum {
    fn default() -> Self {
        Self {
            return_items: vec![],
        }
    }
}

impl ReadWriteDatum {
    pub fn byte_len(&self) -> usize {
        if self.return_items.is_empty() {
            0
        } else {
            let mut sum = 0;
            for i in 0..self.return_items.len() {
                let item_byte_len = self.return_items.get(i).unwrap().byte_len();
                sum += item_byte_len;
                // 当数据不是最后一个的时候，如果数据长度为奇数，S7协议会多填充一个字节，使其数量保持为偶数（最后一个奇数长度数据不需要填充）
                if i != self.return_items.len() - 1 && item_byte_len % 2 == 1 {
                    sum += 1;
                }
            }
            sum
        }
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        if self.return_items.is_empty() {
            return vec![];
        }
        let mut buf = BytesMut::new();
        for i in 0..self.return_items.len() {
            let item_byte_len = self.return_items.get(i).unwrap().byte_len();
            buf.extend_from_slice(&self.return_items.get(i).unwrap().to_be_bytes());
            // 当数据不是最后一个的时候，如果数据长度为奇数，S7协议会多填充一个字节，使其数量保持为偶数（最后一个奇数长度数据不需要填充）
            if i != self.return_items.len() - 1 && item_byte_len % 2 == 1 {
                buf.put_u8(0x00);
            }
        }
        buf.to_vec()
    }

    /// 解析 ReadWriteDatum
    pub fn from_be_bytes(
        data: &[u8],
        message_type: S7MessageType,
        function_code: S7FunctionCode,
    ) -> Result<Self, S7Error> {
        let mut items = Vec::new();
        let mut offset = 0;
        while offset < data.len() {
            let remain = &data[offset..];
            match (message_type, function_code) {
                (S7MessageType::AckData, S7FunctionCode::WriteVariable) => {
                    // 写操作的响应：每个项为 ReturnItem
                    let item = ReturnItem::from_be_bytes(remain)?;
                    offset += item.byte_len();
                    items.push(S7ReturnItem::Return(item));
                }
                _ => {
                    // 读操作的响应：每个项为 DataItem，带奇数填充
                    let item = DataItem::from_be_bytes(remain)?;
                    let item_len = item.byte_len();
                    items.push(S7ReturnItem::Data(item));
                    offset += item_len;
                    // 如果不是最后一个且 item_len 为奇数，跳过填充字节
                    if offset < data.len() && item_len % 2 == 1 {
                        if offset >= data.len() {
                            break;
                        }
                        // 确认填充字节应为 0x00
                        if data[offset] != 0x00 {
                            // 不做强制检查，仅跳过
                        }
                        offset += 1;
                    }
                }
            }
        }
        Ok(ReadWriteDatum {
            return_items: items,
        })
    }
}

#[derive(Debug, Clone)]
pub enum Datum {
    ReadWrite(ReadWriteDatum),
    Szl(SzlDaum),
}

impl Default for Datum {
    fn default() -> Self {
        Datum::ReadWrite(Default::default())
    }
}

impl Datum {
    pub fn byte_len(&self) -> usize {
        match self {
            Datum::ReadWrite(d) => d.byte_len(),
            Datum::Szl(d) => d.byte_len(),
        }
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        match self {
            Datum::ReadWrite(rw) => rw.to_be_bytes(),
            Datum::Szl(d) => d.to_be_bytes(),
        }
    }

    pub fn from_be_bytes(
        bytes: &[u8],
        message_type: S7MessageType,
        function_code: S7FunctionCode,
    ) -> Result<Self, S7Error> {
        match function_code {
            S7FunctionCode::CpuServices => Ok(Datum::Szl(SzlDaum::from_be_bytes(bytes)?)),
            S7FunctionCode::ReadVariable | S7FunctionCode::WriteVariable => Ok(Datum::ReadWrite(
                ReadWriteDatum::from_be_bytes(bytes, message_type, function_code)?,
            )),
            _ => Err(S7Error::Error(
                "function code can not be recognized".to_string(),
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SzlDaum {
    pub return_code: S7ReturnCode,
    pub transport_size: u8,
    pub data_len: u16,
    pub szl_id: u16,
    pub szl_index: u16,

    pub partlist_byte_len: Option<u16>,
    pub partlist_count: Option<u16>,
    pub all_data: Option<Vec<u8>>,
}

impl SzlDaum {
    pub fn new(szl_id: u16, szl_index: u16) -> Self {
        SzlDaum {
            return_code: S7ReturnCode::Success,
            transport_size: 0x09,
            data_len: 0x0004,
            szl_id,
            szl_index,
            partlist_byte_len: None,
            partlist_count: None,
            all_data: None,
        }
    }

    pub fn byte_len(&self) -> usize {
        if let (Some(_partlist_byte_len), Some(_partlist_count), Some(all_data)) = (
            self.partlist_byte_len,
            self.partlist_count,
            self.all_data.clone(),
        ) {
            8 + 2 + 2 + all_data.len()
        } else {
            8
        }
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        buf.put_u8(self.return_code.code());
        buf.put_u8(self.transport_size);
        buf.put_u16(self.data_len);
        buf.put_u16(self.szl_id);
        buf.put_u16(self.szl_index);
        if let (Some(partlist_byte_len), Some(partlist_count), Some(all_data)) = (
            self.partlist_byte_len,
            self.partlist_count,
            self.all_data.clone(),
        ) {
            buf.put_u16(partlist_byte_len);
            buf.put_u16(partlist_count);
            buf.extend_from_slice(all_data.as_slice());
        }
        buf.to_vec()
    }

    pub fn from_be_bytes(data: &[u8]) -> Result<Self, S7Error> {
        let mut buf = BytesMut::from(data);
        let return_code = buf.get_u8();
        let transport_size = buf.get_u8();
        let data_len = buf.get_u16();
        let szl_id = buf.get_u16();
        let szl_index = buf.get_u16();

        let (partlist_byte_len, partlist_count, all_data) = if buf.remaining() >= 4 {
            let partlist_byte_len = buf.get_u16();
            let partlist_count = buf.get_u16();
            let all_data = buf.to_vec();
            (
                Some(partlist_byte_len),
                Some(partlist_count),
                Some(all_data),
            )
        } else {
            (None, None, None)
        };

        Ok(Self {
            return_code: S7ReturnCode::from(return_code).ok_or_else(|| {
                S7Error::Error(format!("Unknown S7ReturnCode: 0x{:02X}", return_code))
            })?,
            transport_size,
            data_len,
            szl_id,
            szl_index,
            partlist_byte_len,
            partlist_count,
            all_data,
        })
    }
}
