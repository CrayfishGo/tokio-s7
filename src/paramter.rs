use crate::error::S7Error;
use crate::item::RequestItem;
use crate::types::{S7FunctionCode, SyntaxID};
use tokio_util::bytes::{Buf, BufMut, BytesMut};

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum S7Parameter {
    SetupParameter(SetupComParameter),
    ReadWriteParameter(ReadWriteParameter),
}

impl Default for S7Parameter {
    fn default() -> S7Parameter {
        S7Parameter::ReadWriteParameter(ReadWriteParameter::default())
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct SetupComParameter {
    /**
     * Function code.
     * 功能码 <br>
     * 字节大小：1 <br>
     * 字节序数：0
     */
    pub function_code: S7FunctionCode,

    /**
     * Reserved.
     * 预留 <br>
     * 字节大小：1 <br>
     * 字节序数：1
     */
    pub reserved: u8,

    /**
     * Max amq caller.
     * Ack队列的大小（主叫）（大端）<br>
     * 字节大小：2 <br>
     * 字节序数：2-3
     */
    pub max_amq_caller: u16,

    /**
     * Max amq callee
     * Ack队列的大小（被叫）（大端）<br>
     * 字节大小：2 <br>
     * 字节序数：4-5
     */
    pub max_amq_callee: u16,

    /**
     * PDU length.
     * PDU长度（大端）<br>
     * 字节大小：2 <br>
     * 字节序数：6-7
     */
    pub pdu_length: u16,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct ReadWriteParameter {
    /**
     * Function code.
     * 功能码 <br>
     * 字节大小：1 <br>
     * 字节序数：0
     */
    pub function_code: S7FunctionCode,

    /**
     * Item count.
     * Request Item结构的数量 <br>
     * 字节大小：1 <br>
     * 字节序数：1
     */
    pub item_count: u8,

    /**
     * Request items.
     * (可重复的请求项)
     */
    pub request_items: Vec<RequestItem>,
}

impl ReadWriteParameter {
    pub fn new(func_code: S7FunctionCode, req_items: &mut Vec<RequestItem>) -> ReadWriteParameter {
        let mut p = Self {
            function_code: func_code,
            item_count: 0,
            request_items: vec![],
        };
        p.add_request_items(req_items);
        p
    }
}

impl Default for ReadWriteParameter {
    fn default() -> ReadWriteParameter {
        Self {
            function_code: S7FunctionCode::ReadVariable,
            item_count: 0,
            request_items: vec![],
        }
    }
}

impl ReadWriteParameter {
    pub fn add_request_item(&mut self, request_item: RequestItem) {
        self.request_items.push(request_item);
        self.item_count = self.request_items.len() as u8;
    }

    pub fn add_request_items(&mut self, request_item: &mut Vec<RequestItem>) {
        self.request_items.append(request_item);
        self.item_count = self.request_items.len() as u8;
    }

    pub fn byte_length(&self) -> u16 {
        2 + self
            .request_items
            .iter()
            .map(|i| i.to_be_bytes().len() as u16)
            .sum::<u16>()
    }
}

impl Default for SetupComParameter {
    fn default() -> SetupComParameter {
        SetupComParameter {
            function_code: S7FunctionCode::SetupCommunication,
            reserved: 0x00,
            max_amq_caller: 1,
            max_amq_callee: 1,
            pdu_length: 240,
        }
    }
}

impl SetupComParameter {
    pub const BYTE_LENGTH: usize = 8;

    pub fn new(pdu_length: u16) -> SetupComParameter {
        Self {
            pdu_length,
            ..SetupComParameter::default()
        }
    }
}

impl S7Parameter {
    pub fn function_code(&self) -> S7FunctionCode {
        match self {
            S7Parameter::SetupParameter(p) => p.function_code,
            S7Parameter::ReadWriteParameter(p) => p.function_code,
        }
    }

    pub fn byte_len(&self) -> usize {
        match self {
            S7Parameter::SetupParameter(_) => SetupComParameter::BYTE_LENGTH,
            S7Parameter::ReadWriteParameter(p) => p.byte_length() as usize,
        }
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        match self {
            S7Parameter::SetupParameter(p) => {
                let mut buf = BytesMut::with_capacity(SetupComParameter::BYTE_LENGTH);
                buf.put_u8(p.function_code.code());
                buf.put_u8(p.reserved);
                buf.put_u16(p.max_amq_callee);
                buf.put_u16(p.max_amq_callee);
                buf.put_u16(p.pdu_length);
                buf.to_vec()
            }
            S7Parameter::ReadWriteParameter(p) => {
                let mut buf = BytesMut::with_capacity(p.byte_length() as usize);
                buf.put_u8(p.function_code.code());
                buf.put_u8(p.item_count);
                p.request_items.iter().for_each(|i| {
                    buf.extend_from_slice(&i.to_be_bytes());
                });
                buf.to_vec()
            }
        }
    }

    pub fn from_be_bytes(data: &[u8]) -> Result<Option<Self>, S7Error> {
        let mut buf = BytesMut::from(data);
        let func_code = data[0];
        let function_code = S7FunctionCode::from(func_code)
            .ok_or_else(|| S7Error::Error(format!("Unknown S7FunctionCode: 0x{:02X}", func_code)))?;
        let s7_param = match function_code {
            S7FunctionCode::SetupCommunication => {
                let _fc = buf.get_u8();
                S7Parameter::SetupParameter(SetupComParameter {
                    function_code,
                    reserved: buf.get_u8(),
                    max_amq_caller: buf.get_u16(),
                    max_amq_callee: buf.get_u16(),
                    pdu_length: buf.get_u16(),
                })
            }
            S7FunctionCode::ReadVariable | S7FunctionCode::WriteVariable => {
                let _fc = buf.get_u8();
                let item_count = buf.get_u8();
                let mut rw = ReadWriteParameter {
                    function_code,
                    item_count,
                    request_items: vec![],
                };
                if item_count == 0 || data.len() == 2 {
                    S7Parameter::ReadWriteParameter(rw)
                } else {
                    let mut offset = 2usize;
                    for _i in 0..item_count {
                        let bytes = &data[offset..];
                        let syntax_id = SyntaxID::from(bytes[2]).ok_or_else(|| {
                            S7Error::Error(format!("Unknown SyntaxID: 0x{:02X}", bytes[2]))
                        })?;
                        if syntax_id == SyntaxID::S7Any {
                            let ri = RequestItem::from_be_bytes_offset(data, offset)?;
                            rw.add_request_item(ri);
                        } else {
                            return Err(S7Error::Error(format!(
                                "Unsupported SyntaxID: 0x{:02X} for function: 0x{:02X}",
                                bytes[2], func_code
                            )));
                        }
                        offset += 12;
                    }
                    S7Parameter::ReadWriteParameter(rw)
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(s7_param))
    }
}
