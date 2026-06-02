/// PLC 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlcType {
    S200,
    S200Smart,
    S300,
    S400,
    S1200,
    S1500,
    Sinumerik828D,
}

impl PlcType{
    pub fn default_rack_slot(&self) -> (u8, u8) {
        match self {
            Self::S200 => (0, 1),
            Self::S200Smart => (0, 1),
            Self::S300 => (0, 2),
            Self::S400 => (0, 3),
            Self::S1200 => (0, 1),
            Self::S1500 => (0, 1),
            Self::Sinumerik828D => (0, 2),
        }
    }

    /// Get PDU protocol type
    pub fn protocol_type(&self) -> u16 {
        match self {
            Self::S200 | Self::S200Smart => 0x01, // ISOTCP243
            _ => 0x02, // ISOTCP
        }
    }

    /// Get default PDU size
    pub fn default_pdu_size(&self) -> u16 {
        match self {
            Self::S200 => 240,
            Self::S200Smart => 480,
            Self::S300 => 480,
            Self::S400 => 1140,
            Self::S1200 => 480,
            Self::S1500 => 1140,
            Self::Sinumerik828D => 480,
        }
    }
}

/// PDU类型（CR/CC/DR/DC/DT等）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PduType {
    ConnectRequest = 0xE0,
    ConnectConfirm = 0xD0,
    DisconnectRequest = 0x80,
    DisconnectConfirm = 0xC0,
    Reject = 0x50,
    DtData = 0xF0,
}

impl PduType {

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from(code: u8) -> Option<Self> {
        match code {
            0xE0 => Some(Self::ConnectRequest),
            0xD0 => Some(Self::ConnectConfirm),
            0x80 => Some(Self::DisconnectRequest),
            0xC0 => Some(Self::DisconnectConfirm),
            0x50 => Some(Self::Reject),
            0xF0 => Some(Self::DtData),
            _ => None,
        }
    }
}

impl TryFrom<u8> for PduType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        PduType::from(value).ok_or(())
    }
}

/// S7 数据区域
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum S7Area {
    /// 200系列系统信息
    SystemInfoOf200Family = 0x03,
    /// 200系列系统标志 (SM)
    SystemFlagsOf200Family = 0x05,
    /// 200系列模拟量输入 (AI)
    AnalogInputsOf200Family = 0x06,
    /// 200系列模拟量输出 (AQ)
    AnalogOutputsOf200Family = 0x07,
    /// 直接访问外设 (PI/PQ)
    // 注意：Java 中 0x80 对应 byte 的 -128
    DirectPeripheralAccess = 0x80,
    /// 输入 (I)
    Inputs = 0x81,
    /// 输出 (Q)
    Outputs = 0x82,
    /// 内部标志 (M)
    Flags = 0x83,
    /// 数据块 (DB)
    DataBlocks = 0x84,
    /// 背景数据块 (DI)
    InstanceDataBlocks = 0x85,
    /// 局部变量 (L)
    LocalData = 0x86,
    /// 全局变量 (V)
    UnknownYet = 0x87,
    /// S7计数器 (C)
    S7Counters = 0x1C,
    /// S7定时器 (T)
    S7Timers = 0x1D,
    /// IEC计数器 (200系列)
    IecCounters = 0x1E,
    /// IEC定时器 (200系列)
    IecTimers = 0x1F,
}

impl S7Area {

    pub fn code(self) -> u8 {
        self as u8
    }

    /// 获取区域对应的缩写字符串
    pub fn abbr(&self) -> &'static str {
        match self {
            S7Area::SystemInfoOf200Family => "",
            S7Area::SystemFlagsOf200Family => "SM",
            S7Area::AnalogInputsOf200Family => "AI",
            S7Area::AnalogOutputsOf200Family => "AQ",
            S7Area::DirectPeripheralAccess => "PI/PQ",
            S7Area::Inputs => "I",
            S7Area::Outputs => "Q",
            S7Area::Flags => "M",
            S7Area::DataBlocks => "DB",
            S7Area::InstanceDataBlocks => "DI",
            S7Area::LocalData => "L",
            S7Area::UnknownYet => "V",
            S7Area::S7Counters => "C",
            S7Area::S7Timers => "T",
            S7Area::IecCounters => "",
            S7Area::IecTimers => "",
        }
    }

    /// 根据字节码获取对应的枚举值（等价于 Java 的 from(byte)）
    pub fn from(code: u8) -> Option<Self> {
        // 使用 match 匹配所有有效码值
        match code {
            0x03 => Some(S7Area::SystemInfoOf200Family),
            0x05 => Some(S7Area::SystemFlagsOf200Family),
            0x06 => Some(S7Area::AnalogInputsOf200Family),
            0x07 => Some(S7Area::AnalogOutputsOf200Family),
            0x80 => Some(S7Area::DirectPeripheralAccess), // 即 0x80
            0x81 => Some(S7Area::Inputs),                 // 0x81
            0x82 => Some(S7Area::Outputs),                // 0x82
            0x83 => Some(S7Area::Flags),                  // 0x83
            0x84 => Some(S7Area::DataBlocks),             // 0x84
            0x85 => Some(S7Area::InstanceDataBlocks),     // 0x85
            0x86 => Some(S7Area::LocalData),              // 0x86
            0x87 => Some(S7Area::UnknownYet),             // 0x87
            0x1C => Some(S7Area::S7Counters),
            0x1D => Some(S7Area::S7Timers),
            0x1E => Some(S7Area::IecCounters),
            0x1F => Some(S7Area::IecTimers),
            _ => None,
        }
    }

    /// 从首字符解析区域
    pub(crate) fn from_first_char(c: char) -> Option<Self> {
        match c {
            'I' => Some(S7Area::Inputs),
            'Q' => Some(S7Area::Outputs),
            'M' => Some(S7Area::Flags),
            'D' | 'V' => Some(S7Area::DataBlocks),
            'T' => Some(S7Area::S7Timers),
            'C' => Some(S7Area::S7Counters),
            _ => None,
        }
    }
}

// 可选：实现 TryFrom trait，使转换更符合 Rust 习惯
impl TryFrom<u8> for S7Area {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        S7Area::from(value).ok_or(())
    }
}

/// S7 数据变量类型（变量类型和长度信息）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum S7DataVariableType {
    /// 无
    Null = 0x00,
    /// 位访问，长度以位为单位
    Bit = 0x03,
    /// 字节/字/双字访问，长度以位为单位
    ByteWordDword = 0x04,
    /// 整数访问，长度以位为单位
    Integer = 0x05,
    /// 双整数访问，长度以字节为单位
    DInteger = 0x06,
    /// 实数访问，长度以字节为单位
    Real = 0x07,
    /// 八位位组字符串，长度以字节为单位
    OctetString = 0x09,
}

impl S7DataVariableType {
    /// 获取变量类型对应的字节码（与 Java 的 getCode() 等效）
    pub fn code(self) -> u8 {
        self as u8
    }

    /// 根据字节码获取对应的枚举值（与 Java 的 from(byte) 等效）
    pub fn from(code: u8) -> Option<Self> {
        match code {
            0x00 => Some(S7DataVariableType::Null),
            0x03 => Some(S7DataVariableType::Bit),
            0x04 => Some(S7DataVariableType::ByteWordDword),
            0x05 => Some(S7DataVariableType::Integer),
            0x06 => Some(S7DataVariableType::DInteger),
            0x07 => Some(S7DataVariableType::Real),
            0x09 => Some(S7DataVariableType::OctetString),
            _ => None,
        }
    }
}

// 实现 TryFrom，支持通过 .try_into() 转换
impl TryFrom<u8> for S7DataVariableType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        S7DataVariableType::from(value).ok_or(())
    }
}

/// S7 错误类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum S7ErrorClass {
    /// 没有错误
    NoError = 0x00,
    /// 应用关系
    ApplicationRelationship = 0x81,
    /// 对象定义
    ObjectDefinition = 0x82,
    /// 没有可用资源
    NoResourcesAvailable = 0x83,
    /// 服务处理中错误
    ErrorOnServiceProcessing = 0x84,
    /// 请求错误
    ErrorOnSupplies = 0x85,
    /// 访问错误
    AccessError = 0x87,
    /// 下载错误
    DownloadError = 0xD2,
}

impl S7ErrorClass {
    /// 获取错误类型的字节码（u8）
    pub fn code(self) -> u8 {
        self as u8
    }

    /// 获取错误描述字符串
    pub fn description(self) -> &'static str {
        match self {
            S7ErrorClass::NoError => "no error",
            S7ErrorClass::ApplicationRelationship => "application relationship",
            S7ErrorClass::ObjectDefinition => "object definition",
            S7ErrorClass::NoResourcesAvailable => "no resources available",
            S7ErrorClass::ErrorOnServiceProcessing => "error on service processing",
            S7ErrorClass::ErrorOnSupplies => "error on supplies",
            S7ErrorClass::AccessError => "access error",
            S7ErrorClass::DownloadError => "download error",
        }
    }

    /// 根据字节码获取对应的枚举值
    pub fn from(code: u8) -> Option<Self> {
        match code {
            0x00 => Some(S7ErrorClass::NoError),
            0x81 => Some(S7ErrorClass::ApplicationRelationship),
            0x82 => Some(S7ErrorClass::ObjectDefinition),
            0x83 => Some(S7ErrorClass::NoResourcesAvailable),
            0x84 => Some(S7ErrorClass::ErrorOnServiceProcessing),
            0x85 => Some(S7ErrorClass::ErrorOnSupplies),
            0x87 => Some(S7ErrorClass::AccessError),
            0xD2 => Some(S7ErrorClass::DownloadError),
            _ => None,
        }
    }
}

// 实现 TryFrom，支持通过 .try_into() 转换
impl TryFrom<u8> for S7ErrorClass {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        S7ErrorClass::from(value).ok_or(())
    }
}

/// S7 功能码
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum S7FunctionCode {
    /// CPU服务
    CpuServices = 0x00,
    /// 读变量
    ReadVariable = 0x04,
    /// 写变量
    WriteVariable = 0x05,
    /// 开始下载
    StartDownload = 0xFA,
    /// 下载阻塞
    Download = 0xFB,
    /// 下载结束
    EndDownload = 0xFC,
    /// 开始上传
    StartUpload = 0x1D,
    /// 上传
    Upload = 0x1E,
    /// 结束上传
    EndUpload = 0x1F,
    /// 控制PLC
    PlcControl = 0x28,
    /// 停止PLC
    PlcStop = 0x29,
    /// 设置通信
    SetupCommunication = 0xF0,
}

impl S7FunctionCode {
    /// 获取字节码
    pub fn code(self) -> u8 {
        self as u8
    }

    /// 根据字节码获取枚举值
    pub fn from(code: u8) -> Option<Self> {
        match code {
            0x00 => Some(S7FunctionCode::CpuServices),
            0x04 => Some(S7FunctionCode::ReadVariable),
            0x05 => Some(S7FunctionCode::WriteVariable),
            0xFA => Some(S7FunctionCode::StartDownload),
            0xFB => Some(S7FunctionCode::Download),
            0xFC => Some(S7FunctionCode::EndDownload),
            0x1D => Some(S7FunctionCode::StartUpload),
            0x1E => Some(S7FunctionCode::Upload),
            0x1F => Some(S7FunctionCode::EndUpload),
            0x28 => Some(S7FunctionCode::PlcControl),
            0x29 => Some(S7FunctionCode::PlcStop),
            0xF0 => Some(S7FunctionCode::SetupCommunication),
            _ => None,
        }
    }
}

/// S7 消息类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum S7MessageType {
    /// 作业请求
    Job = 0x01,
    /// 确认（无数据字段）
    Ack = 0x02,
    /// 从设备回应主设备的作业
    AckData = 0x03,
    /// 用户数据（协议扩展）
    UserData = 0x07,
}

impl S7MessageType {
    /// 获取字节码
    pub fn code(self) -> u8 {
        self as u8
    }

    /// 根据字节码获取枚举值
    pub fn from(code: u8) -> Option<Self> {
        match code {
            0x01 => Some(S7MessageType::Job),
            0x02 => Some(S7MessageType::Ack),
            0x03 => Some(S7MessageType::AckData),
            0x07 => Some(S7MessageType::UserData),
            _ => None,
        }
    }
}

/// S7 参数变量类型（Item数据中的传输尺寸）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum S7ParamVariableType {
    /// 位
    Bit = 0x01,
    /// 字节
    Byte = 0x02,
    /// 字符
    Char = 0x03,
    /// 字
    Word = 0x04,
    /// 整数
    Int = 0x05,
    /// 双字
    Dword = 0x06,
    /// 双整数
    Dint = 0x07,
    /// 浮点
    Real = 0x08,
    /// 日期
    Date = 0x09,
    /// 时间（当日时间）
    Tod = 0x0A,
    /// 时间（持续时间）
    Time = 0x0B,
    /// S5时间
    S5Time = 0x0C,
    /// 日期和时间
    DateAndTime = 0x0F,
    /// 计数器
    Counter = 0x1C,
    /// 定时器
    Timer = 0x1D,
    /// IEC定时器
    IecTimer = 0x1E,
    /// IEC计数器
    IecCounter = 0x1F,
    /// 高速计数器
    HsCounter = 0x20,
}

impl S7ParamVariableType {
    /// 获取字节码
    pub fn code(self) -> u8 {
        self as u8
    }

    /// 根据字节码获取枚举值
    pub fn from(code: u8) -> Option<Self> {
        match code {
            0x01 => Some(S7ParamVariableType::Bit),
            0x02 => Some(S7ParamVariableType::Byte),
            0x03 => Some(S7ParamVariableType::Char),
            0x04 => Some(S7ParamVariableType::Word),
            0x05 => Some(S7ParamVariableType::Int),
            0x06 => Some(S7ParamVariableType::Dword),
            0x07 => Some(S7ParamVariableType::Dint),
            0x08 => Some(S7ParamVariableType::Real),
            0x09 => Some(S7ParamVariableType::Date),
            0x0A => Some(S7ParamVariableType::Tod),
            0x0B => Some(S7ParamVariableType::Time),
            0x0C => Some(S7ParamVariableType::S5Time),
            0x0F => Some(S7ParamVariableType::DateAndTime),
            0x1C => Some(S7ParamVariableType::Counter),
            0x1D => Some(S7ParamVariableType::Timer),
            0x1E => Some(S7ParamVariableType::IecTimer),
            0x1F => Some(S7ParamVariableType::IecCounter),
            0x20 => Some(S7ParamVariableType::HsCounter),
            _ => None,
        }
    }
}

/// S7 操作返回码
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum S7ReturnCode {
    /// 未定义，预留
    Reserved = 0x00,
    /// 硬件错误
    HardwareError = 0x01,
    /// 对象不允许访问
    AccessingTheObjectNotAllowed = 0x03,
    /// 无效地址，所需的地址超出此PLC的极限
    InvalidAddress = 0x05,
    /// 数据类型不支持
    DataTypeNotSupported = 0x06,
    /// 数据类型不一致
    DataTypeInconsistent = 0x07,
    /// 对象不存在
    ObjectDoesNotExist = 0x0A,
    /// 成功
    Success = 0xFF,
}

impl S7ReturnCode {
    /// 获取字节码
    pub fn code(self) -> u8 {
        self as u8
    }

    /// 获取描述信息
    pub fn description(self) -> &'static str {
        match self {
            S7ReturnCode::Reserved => "reserved",
            S7ReturnCode::HardwareError => "hardware error",
            S7ReturnCode::AccessingTheObjectNotAllowed => "accessing the object not allowed",
            S7ReturnCode::InvalidAddress => "invalid address",
            S7ReturnCode::DataTypeNotSupported => "data type not supported",
            S7ReturnCode::DataTypeInconsistent => "data type inconsistent",
            S7ReturnCode::ObjectDoesNotExist => "object does not exist",
            S7ReturnCode::Success => "success",
        }
    }

    /// 根据字节码获取枚举值
    pub fn from(code: u8) -> Option<Self> {
        match code {
            0x00 => Some(S7ReturnCode::Reserved),
            0x01 => Some(S7ReturnCode::HardwareError),
            0x03 => Some(S7ReturnCode::AccessingTheObjectNotAllowed),
            0x05 => Some(S7ReturnCode::InvalidAddress),
            0x06 => Some(S7ReturnCode::DataTypeNotSupported),
            0x07 => Some(S7ReturnCode::DataTypeInconsistent),
            0x0A => Some(S7ReturnCode::ObjectDoesNotExist),
            0xFF => Some(S7ReturnCode::Success),
            _ => None,
        }
    }
}

/// S7 寻址模式与项结构格式标识
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SyntaxID {
    /// Address data S7-Any pointer-like DB1.DBX10.2
    S7Any = 0x10,
    /// R_ID for PBC
    PbcRId = 0x13,
    /// Alarm lock/free dataset
    AlarmLockfree = 0x15,
    /// Alarm indication dataset
    AlarmInd = 0x16,
    /// Alarm acknowledge message dataset
    AlarmAck = 0x19,
    /// Alarm query request dataset
    AlarmQueryreq = 0x1A,
    /// Notify indication dataset
    NotifyInd = 0x1C,
    /// DRIVEESANY seen on Drive ES Starter with routing over S7
    DriveEsAny = 0xA2,
    /// Symbolic byte_address mode of S7-1200
    S1200Sym = 0xB2,
    /// Kind of DB block read, seen only at an S7-400
    DbRead = 0xB0,
    /// Sinumerik NCK HMI access
    Nck = 0x82,
}

impl SyntaxID {
    /// 获取字节码
    pub fn code(self) -> u8 {
        self as u8
    }

    /// 根据字节码获取枚举值
    pub fn from(code: u8) -> Option<Self> {
        match code {
            0x10 => Some(SyntaxID::S7Any),
            0x13 => Some(SyntaxID::PbcRId),
            0x15 => Some(SyntaxID::AlarmLockfree),
            0x16 => Some(SyntaxID::AlarmInd),
            0x19 => Some(SyntaxID::AlarmAck),
            0x1A => Some(SyntaxID::AlarmQueryreq),
            0x1C => Some(SyntaxID::NotifyInd),
            0xA2 => Some(SyntaxID::DriveEsAny),
            0xB2 => Some(SyntaxID::S1200Sym),
            0xB0 => Some(SyntaxID::DbRead),
            0x82 => Some(SyntaxID::Nck),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code() {
        assert_eq!(S7DataVariableType::Null.code(), 0x00);
        assert_eq!(S7DataVariableType::Bit.code(), 0x03);
        assert_eq!(S7DataVariableType::ByteWordDword.code(), 0x04);
        assert_eq!(S7DataVariableType::Integer.code(), 0x05);
        assert_eq!(S7DataVariableType::DInteger.code(), 0x06);
        assert_eq!(S7DataVariableType::Real.code(), 0x07);
        assert_eq!(S7DataVariableType::OctetString.code(), 0x09);
    }

    #[test]
    fn test_from() {
        assert_eq!(
            S7DataVariableType::from(0x00),
            Some(S7DataVariableType::Null)
        );
        assert_eq!(
            S7DataVariableType::from(0x03),
            Some(S7DataVariableType::Bit)
        );
        assert_eq!(
            S7DataVariableType::from(0x04),
            Some(S7DataVariableType::ByteWordDword)
        );
        assert_eq!(
            S7DataVariableType::from(0x05),
            Some(S7DataVariableType::Integer)
        );
        assert_eq!(
            S7DataVariableType::from(0x06),
            Some(S7DataVariableType::DInteger)
        );
        assert_eq!(
            S7DataVariableType::from(0x07),
            Some(S7DataVariableType::Real)
        );
        assert_eq!(
            S7DataVariableType::from(0x09),
            Some(S7DataVariableType::OctetString)
        );
        assert_eq!(S7DataVariableType::from(0xFF), None);
    }

    #[test]
    fn test_try_from() {
        let var: S7DataVariableType = 0x03u8.try_into().unwrap();
        assert_eq!(var, S7DataVariableType::Bit);
    }

    #[test]
    fn test_abbr() {
        assert_eq!(S7Area::Inputs.abbr(), "I");
        assert_eq!(S7Area::DataBlocks.abbr(), "DB");
        assert_eq!(S7Area::DirectPeripheralAccess.abbr(), "PI/PQ");
    }

    #[test]
    fn test_from_s7area() {
        assert_eq!(S7Area::from(0x80), Some(S7Area::DirectPeripheralAccess));
        assert_eq!(S7Area::from(0x1C), Some(S7Area::S7Counters));
        assert_eq!(S7Area::from(0x99), None);
    }
}
