use crate::error::S7Error;
use crate::item::{DataItem, ReturnItem, S7ReturnItem};
use crate::types::{S7FunctionCode, S7MessageType};
use tokio_util::bytes::{BufMut, BytesMut};

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
}

impl Default for Datum {
    fn default() -> Self {
        Datum::ReadWrite(Default::default())
    }
}

impl Datum {
    pub fn byte_len(&self) -> usize {
        match self {
            Datum::ReadWrite(rw) => rw.byte_len(),
        }
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        match self {
            Datum::ReadWrite(rw) => rw.to_be_bytes(),
        }
    }

    pub fn from_be_bytes(
        bytes: &[u8],
        message_type: S7MessageType,
        function_code: S7FunctionCode,
    ) -> Result<Self, S7Error> {
        Ok(Datum::ReadWrite(ReadWriteDatum::from_be_bytes(
            bytes,
            message_type,
            function_code,
        )?))
    }
}
