//! BMP 填补字节隐写。BMP 每条扫描行的字节数要对齐到 4 的倍数，`宽*位深/8` 之后不足
//! 的部分是**填补字节**（正常为 0），可藏数据。本节点解析原始 BMP 字节，逐行取出/写入
//! 这些填补字节（`image` crate 解码会丢弃它们，故手工解析）。
use super::prelude::*;

fn u16le(d: &[u8], o: usize) -> u32 {
    d[o] as u32 | (d[o + 1] as u32) << 8
}
fn u32le(d: &[u8], o: usize) -> u32 {
    u16le(d, o) | u16le(d, o + 2) << 16
}

/// 从 BMP 头解析出 (像素起始, 每行有效字节, 每行填补字节, 行数)。
fn layout(d: &[u8]) -> Result<(usize, usize, usize, usize), CoreError> {
    if d.len() < 30 || &d[0..2] != b"BM" {
        return Err(CoreError::Parse("不是 BMP 文件（缺少 'BM' 头）。".into()));
    }
    let off = u32le(d, 10) as usize; // bfOffBits：像素数据偏移
    let width = u32le(d, 18) as i32; // biWidth
    let height = u32le(d, 22) as i32; // biHeight（可负=自上而下）
    let bpp = u16le(d, 28) as usize; // biBitCount
    let compression = u32le(d, 30);
    if compression != 0 {
        return Err(CoreError::Parse(
            "仅支持未压缩 BMP（biCompression=0）。".into(),
        ));
    }
    if width <= 0 || bpp == 0 {
        return Err(CoreError::Parse("BMP 宽度/位深无效。".into()));
    }
    let width = width as usize;
    let rows = height.unsigned_abs() as usize;
    let row_bytes = width.saturating_mul(bpp).div_ceil(8); // 每行有效字节
    let stride = width.saturating_mul(bpp).div_ceil(32) * 4; // 4 字节对齐后的行跨度
    let pad = stride - row_bytes;
    if off + rows * stride > d.len() {
        return Err(CoreError::Parse(
            "像素数据超出文件范围，BMP 可能损坏。".into(),
        ));
    }
    Ok((off, row_bytes, pad, rows))
}

struct Extract;
impl Node for Extract {
    fn run(
        &self,
        i: &PortMap,
        _p: &serde_json::Value,
        _c: &mut NodeCtx,
    ) -> Result<PortMap, CoreError> {
        let d = in_bytes(i, "data")?;
        let (off, row_bytes, pad, rows) = layout(&d)?;
        if pad == 0 {
            return Err(CoreError::Parse(
                "该 BMP 每行无填补字节（宽度已 4 字节对齐），无处可藏。".into(),
            ));
        }
        let stride = row_bytes + pad;
        let mut out = Vec::with_capacity(rows * pad);
        for r in 0..rows {
            let ps = off + r * stride + row_bytes;
            out.extend_from_slice(&d[ps..ps + pad]);
        }
        let report = format!(
            "每行填补 {pad} 字节 × {rows} 行 = {} 字节容量。",
            rows * pad
        );
        let mut m = PortMap::new();
        m.insert(
            "text".into(),
            PortValue::Text(String::from_utf8_lossy(&out).into_owned()),
        );
        m.insert(
            "bytes".into(),
            PortValue::Bytes(Arc::from(out.clone().into_boxed_slice())),
        );
        m.insert("hex".into(), PortValue::Text(hex::encode(&out)));
        m.insert("report".into(), PortValue::Text(report));
        Ok(m)
    }
}

struct Embed;
impl Node for Embed {
    fn run(
        &self,
        i: &PortMap,
        _p: &serde_json::Value,
        _c: &mut NodeCtx,
    ) -> Result<PortMap, CoreError> {
        let mut d = in_bytes(i, "data")?;
        let payload = in_bytes(i, "file")?;
        let (off, row_bytes, pad, rows) = layout(&d)?;
        if pad == 0 {
            return Err(CoreError::Other(
                "该 BMP 每行无填补字节，无法嵌入。请用宽度非 4 对齐的 24 位 BMP。".into(),
            ));
        }
        let cap = rows * pad;
        if payload.len() > cap {
            return Err(CoreError::Other(format!(
                "容量不足：填补区仅 {cap} 字节，载荷 {} 字节。",
                payload.len()
            )));
        }
        let stride = row_bytes + pad;
        let mut idx = 0usize;
        for r in 0..rows {
            let ps = off + r * stride + row_bytes;
            for k in 0..pad {
                d[ps + k] = if idx < payload.len() { payload[idx] } else { 0 };
                idx += 1;
            }
        }
        let mut m = PortMap::new();
        m.insert(
            "bytes".into(),
            PortValue::Bytes(Arc::from(d.into_boxed_slice())),
        );
        m.insert(
            "text".into(),
            PortValue::Text(format!("已写入 {} 字节到填补区。", payload.len())),
        );
        Ok(m)
    }
}

pub fn register(reg: &mut NodeRegistry) {
    reg.register(
        desc(
            "bmp_padding_extract",
            STEG,
            "BMP 填补字节提取",
            PURPLE,
            vec![req("data", "BMP", PortType::Any)],
            vec![
                req("text", "文本", PortType::Text),
                opt("bytes", "字节", PortType::Bytes),
                opt("hex", "hex", PortType::Text),
                opt("report", "信息", PortType::Text),
            ],
            vec![],
        ),
        Arc::new(|| Arc::new(Extract)),
    );
    reg.register(
        desc(
            "bmp_padding_embed",
            STEG,
            "BMP 填补字节嵌入",
            PURPLE,
            vec![
                req("data", "BMP", PortType::Any),
                req("file", "载荷", PortType::Any),
            ],
            vec![
                req("bytes", "BMP字节", PortType::Bytes),
                opt("text", "信息", PortType::Text),
            ],
            vec![],
        ),
        Arc::new(|| Arc::new(Embed)),
    );
}
