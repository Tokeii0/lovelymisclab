//! Invoke-PSImage（peewpw/Invoke-PSImage）提取。每个像素藏 1 字节：**高 4 位在蓝色
//! 通道低半字节、低 4 位在绿色通道低半字节**（红色放随机填充）。行主序读取，还原出
//! 原始 PowerShell 脚本（ASCII）。解码等价于加载器里的
//! `($p.B -band 15)*16 -bor ($p.G -band 15)`。
use super::image_util::{image_out, load_image};
use super::prelude::*;

struct Extract;
impl Node for Extract {
    fn run(
        &self,
        i: &PortMap,
        p: &serde_json::Value,
        _c: &mut NodeCtx,
    ) -> Result<PortMap, CoreError> {
        let img = load_image(i, "data")?;
        let mut out: Vec<u8> = img
            .pixels()
            .map(|px| ((px.0[2] & 0x0F) << 4) | (px.0[1] & 0x0F))
            .collect();
        // 载荷无长度标记，脚本后面是随机填充。PS 脚本是纯可打印 ASCII，故截到**第一个**
        // 非文本字节（即脚本与填充的分界）。
        if pbool(p, "trim", true) {
            let is_text = |b: u8| (0x20..=0x7e).contains(&b) || matches!(b, b'\n' | b'\r' | b'\t');
            if let Some(end) = out.iter().position(|&b| !is_text(b)) {
                out.truncate(end);
            }
        }
        let mut m = PortMap::new();
        m.insert(
            "text".into(),
            PortValue::Text(String::from_utf8_lossy(&out).into_owned()),
        );
        m.insert(
            "bytes".into(),
            PortValue::Bytes(Arc::from(out.into_boxed_slice())),
        );
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
        let mut img = load_image(i, "data")?;
        let payload = in_bytes(i, "file")?;
        if payload.len() > img.pixels().len() {
            return Err(CoreError::Other(format!(
                "图片容量不足：{} 像素 < 载荷 {} 字节。",
                img.pixels().len(),
                payload.len()
            )));
        }
        for (idx, px) in img.pixels_mut().enumerate() {
            if idx >= payload.len() {
                break;
            }
            let byte = payload[idx];
            px.0[2] = (px.0[2] & 0xF0) | (byte >> 4); // B 低半字节 = 高 4 位
            px.0[1] = (px.0[1] & 0xF0) | (byte & 0x0F); // G 低半字节 = 低 4 位
        }
        image_out(&img)
    }
}

pub fn register(reg: &mut NodeRegistry) {
    reg.register(
        desc(
            "psimage_extract",
            STEG,
            "Invoke-PSImage 提取",
            PURPLE,
            vec![req("data", "图片", PortType::Any)],
            vec![
                req("text", "PS 脚本", PortType::Text),
                opt("bytes", "字节", PortType::Bytes),
            ],
            vec![ParamSpec::toggle("trim", "截去尾部随机填充", true)],
        ),
        Arc::new(|| Arc::new(Extract)),
    );
    reg.register(
        desc(
            "psimage_embed",
            STEG,
            "Invoke-PSImage 嵌入",
            PURPLE,
            vec![
                req("data", "载体图片", PortType::Any),
                req("file", "脚本/载荷", PortType::Any),
            ],
            vec![
                req("image", "图片", PortType::Image),
                opt("bytes", "PNG字节", PortType::Bytes),
            ],
            vec![],
        ),
        Arc::new(|| Arc::new(Embed)),
    );
}
