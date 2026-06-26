use crate::error::S7Error;
use crate::types::{S7Area, S7DataVariableType, S7ParamVariableType, S7ReturnCode, SyntaxID};
use tokio_util::bytes::{Buf, BufMut, BytesMut};

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub struct RequestItem {
    /**
     * Specification type.
     * 变量规范，对于读/写消息，它总是具有值0x12 <br>
     * 字节大小：1 <br>
     * 字节序数：0
     */
    pub specification_type: u8,

    /**
     * Length of following.
     * 其余部分的长度规范 <br>
     * 字节大小：1 <br>
     * 字节序数：1
     */
    pub length_of_following: u8,

    /**
     * The format of the addressing mode and the rest of the item structure.
     * 寻址模式和项结构其余部分的格式，它具有任意类型寻址的常量值0x10 <br>
     * 字节大小：1 <br>
     * 字节序数：2
     */
    pub syntax_id: SyntaxID,

    /**
     * Variable type.
     * 变量的类型和长度BIT，BYTE，WORD，DWORD，COUNTER <br>
     * 字节大小：1 <br>
     * 字节序数：3
     */
    pub variable_type: S7ParamVariableType,

    /**
     * Data length.
     * 读取长度 <br>
     * 字节大小：2 <br>
     * 字节序数：4-5
     */
    pub count: u16,

    /**
     * DB number.
     * 即 DB 编号，如果访问的不是DB区域，此处为0x0000 <br>
     * 字节大小：2 <br>
     * 字节序数：6-7
     */
    pub db_number: u16,

    /**
     * Area.
     * 存储区类型DB存储区 <br>
     * 字节大小：1 <br>
     * 字节序数：8
     */
    pub area: S7Area,

    /**
     * Byte address.
     * 字节地址，位于开始字节地址address中3个字节，从第4位开始计数 <br>
     * 字节大小：3 <br>
     * 字节序数：9-11
     */
    pub byte_address: u32,

    /**
     * Bit address.
     * 位地址，位于开始字节地址address中3个字节的最后3位
     */
    pub bit_address: u32,
}

impl RequestItem {
    fn new(
        variable_type: S7ParamVariableType,
        count: u16,
        area: S7Area,
        db_number: u16,
        byte_address: u32,
        bit_address: u32,
    ) -> RequestItem {
        Self {
            specification_type: 0x12,
            length_of_following: 0x0A,
            syntax_id: SyntaxID::S7Any,
            variable_type,
            count,
            db_number,
            area,
            byte_address,
            bit_address,
        }
    }
}

impl RequestItem {
    pub fn parse_bit(address: &str) -> Result<RequestItem, S7Error> {
        Self::parse(address, 1, S7ParamVariableType::Bit)
    }

    pub fn parse_byte(address: &str, count: u16) -> Result<RequestItem, S7Error> {
        Self::parse(address, count, S7ParamVariableType::Byte)
    }

    fn parse(
        address: &str,
        count: u16,
        variable_type: S7ParamVariableType,
    ) -> Result<RequestItem, S7Error> {
        if address.is_empty() {
            return Err(S7Error::Error("address is null or empty".to_string()));
        }
        if count == 0 {
            return Err(S7Error::Error("count must be positive".to_string()));
        }

        let address = address.to_uppercase();
        let parts: Vec<&str> = address.split('.').collect();

        let area = Self::parse_area(&parts)?;
        let db_number = Self::parse_db_number(&parts)?;
        let byte_address = Self::parse_byte_address(&parts)?;
        let bit_address = Self::parse_bit_address(&parts, variable_type)?;

        if bit_address > 7 {
            return Err(S7Error::Error("bit index must be in range 0-7".to_string()));
        }

        Ok(RequestItem::new(
            variable_type,
            count,
            area,
            db_number,
            byte_address,
            bit_address,
        ))
    }

    fn parse_area(parts: &[&str]) -> Result<S7Area, S7Error> {
        let first = parts
            .get(0)
            .ok_or(S7Error::Error("empty address".to_string()))?;
        let c = first
            .chars()
            .next()
            .ok_or(S7Error::Error("empty first part".to_string()))?;
        S7Area::from_first_char(c)
            .ok_or_else(|| S7Error::Error(format!("unknown area prefix: '{}'", c)))
    }

    fn parse_db_number(parts: &[&str]) -> Result<u16, S7Error> {
        let first = parts
            .get(0)
            .ok_or(S7Error::Error("empty address".to_string()))?;
        let c = first.chars().next().unwrap(); // 已经检查过
        match c {
            'D' => Self::extract_number(first),
            'V' => Ok(1), // V 区映射到 DB1
            _ => Ok(0),
        }
    }

    fn parse_byte_address(parts: &[&str]) -> Result<u32, S7Error> {
        let first = parts
            .get(0)
            .ok_or(S7Error::Error("empty address".to_string()))?;
        let c = first.chars().next().unwrap();
        if c == 'D' {
            if parts.len() >= 2 {
                Self::extract_number(parts[1])
            } else {
                Ok(0)
            }
        } else {
            Self::extract_number(first)
        }
    }

    fn parse_bit_address(
        parts: &[&str],
        variable_type: S7ParamVariableType,
    ) -> Result<u32, S7Error> {
        if variable_type != S7ParamVariableType::Bit {
            return Ok(0);
        }
        let first = parts
            .get(0)
            .ok_or(S7Error::Error("empty address".to_string()))?;
        let c = first.chars().next().unwrap();
        if c == 'D' {
            if parts.len() >= 3 {
                Self::extract_number(parts[2]).map(|v| v)
            } else {
                Ok(0)
            }
        } else {
            if parts.len() >= 2 {
                Self::extract_number(parts[1]).map(|v| v)
            } else {
                Ok(0)
            }
        }
    }

    /// 从字符串中提取数字（仅保留 ASCII 数字字符）
    fn extract_number<T: TryFrom<u32>>(s: &str) -> Result<T, S7Error> {
        let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return Err(S7Error::Error(format!("no digits found in '{}'", s)));
        }
        digits
            .parse::<u32>()
            .map_err(|e| S7Error::Error(format!("parse number error: {}", e)))
            .and_then(|n| {
                T::try_from(n).map_err(|_| S7Error::Error(format!("number {} out of range", n)))
            })
    }

    const BYTE_LENGTH: usize = 12;
    pub fn to_be_bytes(&self) -> Vec<u8> {
        let mut bytes = BytesMut::with_capacity(Self::BYTE_LENGTH);
        bytes.put_u8(self.specification_type);
        bytes.put_u8(self.length_of_following);
        bytes.put_u8(self.syntax_id.code());
        bytes.put_u8(self.variable_type.code());
        bytes.put_u16(self.count);
        bytes.put_u16(self.db_number);
        bytes.put_u8(self.area.code());

        // 计算地址部分：定时器/计数器 = byte_address * 4，其他 = byte_address * 8 + bit_address
        let address_bits = if self.area == S7Area::S7Timers || self.area == S7Area::S7Counters {
            self.byte_address << 2
        } else {
            (self.byte_address << 3) + self.bit_address
        };
        // 写入地址的低 24 位（大端）
        bytes.put_u8(((address_bits >> 16) & 0xFF) as u8);
        bytes.put_u8(((address_bits >> 8) & 0xFF) as u8);
        bytes.put_u8((address_bits & 0xFF) as u8);
        bytes.to_vec()
    }

    pub fn from_be_bytes(bytes: &[u8]) -> Result<Self, S7Error> {
        Self::from_be_bytes_offset(bytes, 0)
    }

    pub fn from_be_bytes_offset(bytes: &[u8], offset: usize) -> Result<Self, S7Error> {
        if bytes.len() < offset + 12 {
            // 至少需要12字节
            return Err(S7Error::Error("data too short for RequestItem".into()));
        }
        let mut buf = BytesMut::from(&bytes[offset..]); // 从 offset 开始包装
        let specification_type = buf.get_u8();
        let length_of_following = buf.get_u8();
        let syntax_id = buf.get_u8();
        let variable_type = buf.get_u8();
        let count = buf.get_u16();
        let db_number = buf.get_u16();
        let area = buf.get_u8();

        let addr_high = ((*buf.get(9).unwrap() as u32) & 0xFFu32) << 16;
        let addr_mid = ((*buf.get(10).unwrap() as u32) & 0xFFu32) << 8;
        let addr_low = (*buf.get(11).unwrap() as u32) & 0xFFu32;
        let raw_addr = addr_high | addr_mid | addr_low;

        let (byte_address, bit_address) =
            if area == S7Area::S7Timers.code() || area == S7Area::S7Counters.code() {
                (raw_addr >> 2, 0u32)
            } else {
                (raw_addr >> 3, (raw_addr & 0x07))
            };

        Ok(RequestItem {
            specification_type,
            length_of_following,
            syntax_id: SyntaxID::from(syntax_id)
                .ok_or_else(|| S7Error::Error(format!("Unknown SyntaxID: 0x{:02X}", syntax_id)))?,
            variable_type: S7ParamVariableType::from(variable_type).ok_or_else(|| {
                S7Error::Error(format!(
                    "Unknown S7ParamVariableType: 0x{:02X}",
                    variable_type
                ))
            })?,
            count,
            db_number,
            area: S7Area::from(area)
                .ok_or_else(|| S7Error::Error(format!("Unknown S7Area: 0x{:02X}", area)))?,
            byte_address,
            bit_address,
        })
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub struct ReturnItem {
    /**
     * Return code.
     * 返回码 <br>
     * 字节大小：1 <br>
     * 字节序数：0
     */
    pub return_code: S7ReturnCode,
}

impl ReturnItem {
    pub fn byte_len(&self) -> usize {
        1
    }

    pub fn new(return_code: S7ReturnCode) -> Self {
        ReturnItem { return_code }
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        let mut bytes = BytesMut::new();
        bytes.put_u8(self.return_code.code());
        bytes.to_vec()
    }
    pub fn from_be_bytes(bytes: &[u8]) -> Result<Self, S7Error> {
        Ok(Self {
            return_code: S7ReturnCode::from(bytes[0]).ok_or_else(|| {
                S7Error::Error(format!("Unknown S7ReturnCode: 0x{:02X}", bytes[0]))
            })?,
        })
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct DataItem {
    /**
     * Return code.
     * 返回码 <br>
     * 字节大小：1 <br>
     * 字节序数：0
     */
    pub return_code: S7ReturnCode,

    /**
     * Data variable type.
     * 变量类型 <br>
     * 字节大小：1 <br>
     * 字节序数：1
     */
    pub data_variable_type: S7DataVariableType,

    /**
     * The data length is calculated by bit. If it is byte data, /8 or *8 operation is required to read it. If it is bit data, no additional operation is required.
     * 数据长度，按位进行计算的，如果是字节数据读取需要进行 /8 或 *8操作，如果是位数据，不需要任何额外操作 <br>
     * 字节大小：2 <br>
     * 字节序数：2-3
     */
    pub count: u16,

    /**
     * Data content.
     * 数据内容
     */
    pub data: Vec<u8>,
}

impl DataItem {
    pub fn create_req_bytes(data: Vec<u8>) -> Result<Self, S7Error> {
        if data.is_empty() {
            return Err(S7Error::Error("Empty data".to_string()));
        }
        Ok(DataItem {
            return_code: S7ReturnCode::Reserved,
            data_variable_type: S7DataVariableType::ByteWordDword,
            count: data.len() as u16,
            data,
        })
    }

    pub fn create_req_bool(data: bool) -> Result<Self, S7Error> {
        let byte_value = if data { 0x01u8 } else { 0x00u8 };
        Ok(DataItem {
            return_code: S7ReturnCode::Reserved,
            data_variable_type: S7DataVariableType::Bit,
            count: 1,
            data: vec![byte_value],
        })
    }
}

impl DataItem {
    pub fn byte_len(&self) -> usize {
        4 + self.data.len()
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        let mut bytes = BytesMut::new();
        bytes.put_u8(self.return_code.code());
        bytes.put_u8(self.data_variable_type.code());
        match self.data_variable_type {
            S7DataVariableType::Null
            | S7DataVariableType::ByteWordDword
            | S7DataVariableType::Integer => {
                bytes.put_u16(self.count * 8);
            }
            _ => {
                bytes.put_u16(self.count);
            }
        }
        bytes.extend_from_slice(&self.data);
        bytes.to_vec()
    }

    pub fn from_be_bytes(bytes: &[u8]) -> Result<Self, S7Error> {
        let mut buf = BytesMut::from(bytes);
        let return_code = buf.get_u8();
        let data_variable_type = buf.get_u8();
        let dv = S7DataVariableType::from(data_variable_type).ok_or_else(|| {
            S7Error::Error(format!(
                "Unknown S7DataVariableType: 0x{:02X}",
                data_variable_type
            ))
        })?;

        let item_count = match dv {
            S7DataVariableType::Null
            | S7DataVariableType::ByteWordDword
            | S7DataVariableType::Integer => buf.get_u16() / 8,
            _ => buf.get_u16(),
        };

        let data = if dv != S7DataVariableType::Null {
            if buf.remaining() < item_count as usize {
                return Err(S7Error::Error("DataItem: insufficient data body".into()));
            }
            // 读取 count 个字节到 Vec
            let mut v = vec![0u8; item_count as usize];
            buf.copy_to_slice(&mut v);
            v
        } else {
            Vec::new()
        };

        Ok(DataItem {
            return_code: S7ReturnCode::from(return_code).ok_or_else(|| {
                S7Error::Error(format!("Unknown S7ReturnCode: 0x{:02X}", return_code))
            })?,
            data_variable_type: dv,
            count: item_count,
            data,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum S7ReturnItem {
    Return(ReturnItem),
    Data(DataItem),
}

impl S7ReturnItem {
    pub fn byte_len(&self) -> usize {
        match self {
            S7ReturnItem::Return(r) => r.byte_len(),
            S7ReturnItem::Data(d) => d.byte_len(),
        }
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        match self {
            S7ReturnItem::Return(r) => r.to_be_bytes(),
            S7ReturnItem::Data(d) => d.to_be_bytes(),
        }
    }

    pub fn from_be_bytes(bytes: &[u8]) -> Result<Self, S7Error> {
        if bytes.len() == 1 {
            Ok(S7ReturnItem::Return(ReturnItem::from_be_bytes(bytes)?))
        } else {
            Ok(S7ReturnItem::Data(DataItem::from_be_bytes(bytes)?))
        }
    }
}
