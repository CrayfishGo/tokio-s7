use crate::error::S7Error;
use crate::item::RequestItem;
use crate::types::{S7FunctionCode, SyntaxID};
use tokio_util::bytes::{Buf, BufMut, BytesMut};

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum S7Parameter {
    SetupParameter(SetupComParameter),
    ReadWriteParameter(ReadWriteParameter),
    Szl(SzlParameter),
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

    pub fn byte_len(&self) -> u16 {
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
            pdu_length: 960,
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
            S7Parameter::Szl(p) => p.function_code,
        }
    }

    pub fn byte_len(&self) -> usize {
        match self {
            S7Parameter::SetupParameter(_) => SetupComParameter::BYTE_LENGTH,
            S7Parameter::ReadWriteParameter(p) => p.byte_len() as usize,
            S7Parameter::Szl(p) => p.byte_len(),
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
                let mut buf = BytesMut::with_capacity(p.byte_len() as usize);
                buf.put_u8(p.function_code.code());
                buf.put_u8(p.item_count);
                p.request_items.iter().for_each(|i| {
                    buf.extend_from_slice(&i.to_be_bytes());
                });
                buf.to_vec()
            }
            S7Parameter::Szl(p) => {
                let mut buf = BytesMut::with_capacity(p.byte_len());
                buf.put_u8(p.function_code.code());
                buf.put_u8(p.item_count);
                buf.put_u8(p.varspec);
                buf.put_u8(p.plen);
                buf.put_u8(p.syntax_id.code());
                buf.put_u8(p.func_group);
                buf.put_u8(p.sub_fun);
                buf.put_u8(p.seq);
                buf.to_vec()
            }
        }
    }

    pub fn from_be_bytes(data: &[u8]) -> Result<Option<Self>, S7Error> {
        let mut buf = BytesMut::from(data);
        let func_code = data[0];
        let function_code = S7FunctionCode::from(func_code).ok_or_else(|| {
            S7Error::Error(format!("Unknown S7FunctionCode: 0x{:02X}", func_code))
        })?;
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
            S7FunctionCode::CpuServices => {
                let _fc = buf.get_u8();
                let item_count = buf.get_u8();
                let varspec = buf.get_u8();
                let plen = buf.get_u8();
                let syntax_id = buf.get_u8();
                let func_group = buf.get_u8();
                let sub_fun = buf.get_u8();
                let seq = buf.get_u8();

                let (data_unit_reference_number, last_data_unit, error_code) =
                    if buf.remaining() >= 4 {
                        (Some(buf.get_u8()), Some(buf.get_u8()), Some(buf.get_u16()))
                    } else {
                        (None, None, None)
                    };
                S7Parameter::Szl(SzlParameter {
                    function_code,
                    item_count,
                    varspec,
                    plen,
                    syntax_id: SyntaxID::from(syntax_id).ok_or_else(|| {
                        S7Error::Error(format!("Unknown SyntaxID: 0x{:02X}", syntax_id))
                    })?,
                    func_group,
                    sub_fun,
                    seq,
                    data_unit_reference_number,
                    last_data_unit,
                    error_code,
                })
            }
            _ => return Ok(None),
        };
        Ok(Some(s7_param))
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct SzlParameter {
    pub function_code: S7FunctionCode,
    pub item_count: u8,
    pub varspec: u8,
    pub plen: u8,
    pub syntax_id: SyntaxID,
    pub func_group: u8,
    pub sub_fun: u8,
    pub seq: u8,
    pub data_unit_reference_number: Option<u8>,
    pub last_data_unit: Option<u8>,
    pub error_code: Option<u16>,
}

impl SzlParameter {
    pub fn byte_len(&self) -> usize {
        if let (Some(_data_unit_reference_number), Some(_last_data_unit), Some(_ecd)) = (
            self.data_unit_reference_number,
            self.last_data_unit,
            self.error_code,
        ) {
            12
        } else {
            8
        }
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        buf.put_u8(self.function_code.code());
        buf.put_u8(self.item_count);
        buf.put_u8(self.varspec);
        buf.put_u8(self.plen);
        buf.put_u8(self.syntax_id.code());
        buf.put_u8(self.func_group);
        buf.put_u8(self.sub_fun);
        buf.put_u8(self.seq);
        if let (Some(data_unit_reference_number), Some(last_data_unit), Some(ecd)) = (
            self.data_unit_reference_number,
            self.last_data_unit,
            self.error_code,
        ) {
            buf.put_u8(data_unit_reference_number);
            buf.put_u8(last_data_unit);
            buf.put_u16(ecd);
        }
        buf.to_vec()
    }
}

impl Default for SzlParameter {
    fn default() -> SzlParameter {
        SzlParameter {
            function_code: S7FunctionCode::CpuServices,
            item_count: 0x01,
            varspec: 0x12,
            plen: 0x04,
            syntax_id: SyntaxID::ParameterShort,
            func_group: 0x44,
            sub_fun: 0x01,
            seq: 0x00,
            data_unit_reference_number: None,
            last_data_unit: None,
            error_code: None,
        }
    }
}
