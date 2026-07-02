//! ImageMask（kingthy/imagemask，`imagemask.js`）图片隐写：文本 / 文件两种模式。
//!
//! 在 canvas 的 RGBA 扁平数据上，逐个颜色字节（R、G、B，**跳过 Alpha**）写入
//! `mixCount` 个最低位；数字按 **低位在前** 拆分，每个数字占 `ceil(size/mixCount)`
//! 个颜色字节后对齐到下一字节。
//! - 文本：`[len: lengthSize][char×len: charSize]`，字符为 UTF-16 码元。
//! - 文件：`[nameLen: 8][nameChar×: charSize][dataLen: lengthSize][byte×: 8]`。
//!
//! `charSize`/`lengthSize` 会向上取整为 `mixCount` 的倍数（与原工具一致）。
use image::RgbaImage;

use super::image_util::{image_out, load_image};
use super::prelude::*;

/// 读写游标：offset 走在扁平 RGBA 上，每颜色字节用 mix 个低位，跳过 alpha。
struct Cursor {
    offset: usize,
    mix: usize,
}
impl Cursor {
    fn new(mix: usize) -> Self {
        Self { offset: 0, mix }
    }
    fn skip_alpha(&mut self) {
        // 与 JS 一致：byte 前进后，若落在 alpha（(offset+1)%4==0）则再跳一格。
        if (self.offset + 1).is_multiple_of(4) {
            self.offset += 1;
        }
    }
    fn read_number(&mut self, data: &[u8], size: usize) -> u32 {
        let mut number = 0u32;
        let mut pos = 0usize;
        while pos < size && self.offset < data.len() {
            let mut m = 0;
            while m < self.mix && pos < size {
                let bit = (data[self.offset] >> m) & 1;
                number |= (bit as u32) << pos;
                m += 1;
                pos += 1;
            }
            self.offset += 1;
            self.skip_alpha();
        }
        number
    }
    fn write_number(&mut self, data: &mut [u8], number: u32, size: usize) {
        let mut pos = 0usize;
        while pos < size && self.offset < data.len() {
            let mut m = 0;
            while m < self.mix && pos < size {
                let bit = ((number >> pos) & 1) as u8;
                data[self.offset] = (data[self.offset] & !(1 << m)) | (bit << m);
                m += 1;
                pos += 1;
            }
            self.offset += 1;
            if (self.offset + 1).is_multiple_of(4) && self.offset < data.len() {
                data[self.offset] = 255; // 强制 alpha=255（原工具做法）
                self.offset += 1;
            }
        }
    }
}

/// 向上取整到 mix 的倍数。
fn round(size: usize, mix: usize) -> usize {
    if size.is_multiple_of(mix) {
        size
    } else {
        size + mix - size % mix
    }
}

struct Params {
    mix: usize,
    char_size: usize,
    length_size: usize,
}
fn params(p: &serde_json::Value) -> Params {
    let mix = (pnum(p, "mixCount", 2.0) as usize).clamp(1, 5);
    Params {
        mix,
        char_size: round((pnum(p, "charSize", 16.0) as usize).max(1), mix),
        length_size: round((pnum(p, "lengthSize", 24.0) as usize).max(1), mix),
    }
}

fn cap_bits(img: &RgbaImage, mix: usize) -> usize {
    (img.width() * img.height()) as usize * 3 * mix
}

// ---------------------------------------------------------------- 文本提取
struct TextExtract;
impl Node for TextExtract {
    fn run(
        &self,
        i: &PortMap,
        p: &serde_json::Value,
        _c: &mut NodeCtx,
    ) -> Result<PortMap, CoreError> {
        let img = load_image(i, "data")?;
        let pr = params(p);
        let data = img.into_raw();
        let mut cur = Cursor::new(pr.mix);
        let len = cur.read_number(&data, pr.length_size) as usize;
        if len == 0 || pr.length_size + len * pr.char_size > data.len() / 4 * 3 * pr.mix {
            return Err(CoreError::Parse(
                "未发现 ImageMask 文本（长度无效，试试调整 mixCount）。".into(),
            ));
        }
        let units: Vec<u16> = (0..len)
            .map(|_| cur.read_number(&data, pr.char_size) as u16)
            .collect();
        Ok(out_text(String::from_utf16_lossy(&units)))
    }
}

// ---------------------------------------------------------------- 文件提取
struct FileExtract;
impl Node for FileExtract {
    fn run(
        &self,
        i: &PortMap,
        p: &serde_json::Value,
        _c: &mut NodeCtx,
    ) -> Result<PortMap, CoreError> {
        let img = load_image(i, "data")?;
        let pr = params(p);
        let data = img.into_raw();
        let mut cur = Cursor::new(pr.mix);
        let name_len = cur.read_number(&data, 8) as usize;
        let name_units: Vec<u16> = (0..name_len)
            .map(|_| cur.read_number(&data, pr.char_size) as u16)
            .collect();
        let filename = String::from_utf16_lossy(&name_units);
        let data_len = cur.read_number(&data, pr.length_size) as usize;
        if data_len == 0 || data_len * 8 > cap_bits_from_raw(data.len(), pr.mix) {
            return Err(CoreError::Parse(
                "未发现 ImageMask 文件（长度无效，试试调整 mixCount）。".into(),
            ));
        }
        let bytes: Vec<u8> = (0..data_len)
            .map(|_| cur.read_number(&data, 8) as u8)
            .collect();

        let mut m = PortMap::new();
        m.insert(
            "bytes".into(),
            PortValue::Bytes(Arc::from(bytes.clone().into_boxed_slice())),
        );
        m.insert("filename".into(), PortValue::Text(filename));
        m.insert(
            "text".into(),
            PortValue::Text(String::from_utf8_lossy(&bytes).into_owned()),
        );
        Ok(m)
    }
}

fn cap_bits_from_raw(raw_len: usize, mix: usize) -> usize {
    raw_len / 4 * 3 * mix
}

// ---------------------------------------------------------------- 文本嵌入
struct TextEmbed;
impl Node for TextEmbed {
    fn run(
        &self,
        i: &PortMap,
        p: &serde_json::Value,
        _c: &mut NodeCtx,
    ) -> Result<PortMap, CoreError> {
        let img = load_image(i, "data")?;
        let pr = params(p);
        let text = pstr(p, "text", "");
        let units: Vec<u16> = text.encode_utf16().collect();
        let need = pr.length_size + units.len() * pr.char_size;
        if need > cap_bits(&img, pr.mix) {
            return Err(CoreError::Other("文本过长，图片装不下。".into()));
        }
        let (w, h) = img.dimensions();
        let mut data = img.into_raw();
        let mut cur = Cursor::new(pr.mix);
        cur.write_number(&mut data, units.len() as u32, pr.length_size);
        for u in units {
            cur.write_number(&mut data, u as u32, pr.char_size);
        }
        image_out(&RgbaImage::from_raw(w, h, data).unwrap())
    }
}

// ---------------------------------------------------------------- 文件嵌入
struct FileEmbed;
impl Node for FileEmbed {
    fn run(
        &self,
        i: &PortMap,
        p: &serde_json::Value,
        _c: &mut NodeCtx,
    ) -> Result<PortMap, CoreError> {
        let img = load_image(i, "data")?;
        let pr = params(p);
        let payload = in_bytes(i, "file")?;
        let name_units: Vec<u16> = pstr(p, "filename", "secret.bin")
            .encode_utf16()
            .take(255)
            .collect();
        let need = 8 + name_units.len() * pr.char_size + pr.length_size + payload.len() * 8;
        if need > cap_bits(&img, pr.mix) {
            return Err(CoreError::Other("文件过大，图片装不下。".into()));
        }
        let (w, h) = img.dimensions();
        let mut data = img.into_raw();
        let mut cur = Cursor::new(pr.mix);
        cur.write_number(&mut data, name_units.len() as u32, 8);
        for u in &name_units {
            cur.write_number(&mut data, *u as u32, pr.char_size);
        }
        cur.write_number(&mut data, payload.len() as u32, pr.length_size);
        for b in &payload {
            cur.write_number(&mut data, *b as u32, 8);
        }
        image_out(&RgbaImage::from_raw(w, h, data).unwrap())
    }
}

fn mask_params() -> Vec<ParamSpec> {
    vec![
        ParamSpec::number("mixCount", "混合位数(1-5)", 1.0, 5.0, 1.0, 2.0),
        ParamSpec::number("charSize", "字符位数", 8.0, 32.0, 1.0, 16.0),
        ParamSpec::number("lengthSize", "长度位数", 8.0, 32.0, 1.0, 24.0),
    ]
}

pub fn register(reg: &mut NodeRegistry) {
    reg.register(
        desc(
            "imagemask_text_extract",
            STEG,
            "ImageMask 文本提取",
            PURPLE,
            vec![req("data", "图片", PortType::Any)],
            vec![req("text", "文本", PortType::Text)],
            mask_params(),
        ),
        Arc::new(|| Arc::new(TextExtract)),
    );
    reg.register(
        desc(
            "imagemask_file_extract",
            STEG,
            "ImageMask 文件提取",
            PURPLE,
            vec![req("data", "图片", PortType::Any)],
            vec![
                req("bytes", "文件字节", PortType::Bytes),
                opt("filename", "文件名", PortType::Text),
                opt("text", "文本预览", PortType::Text),
            ],
            mask_params(),
        ),
        Arc::new(|| Arc::new(FileExtract)),
    );
    reg.register(
        desc(
            "imagemask_text_embed",
            STEG,
            "ImageMask 文本嵌入",
            PURPLE,
            vec![req("data", "载体图片", PortType::Any)],
            vec![
                req("image", "图片", PortType::Image),
                opt("bytes", "PNG字节", PortType::Bytes),
            ],
            {
                let mut v = vec![ParamSpec::text("text", "要隐写的文本", "", false)];
                v.extend(mask_params());
                v
            },
        ),
        Arc::new(|| Arc::new(TextEmbed)),
    );
    reg.register(
        desc(
            "imagemask_file_embed",
            STEG,
            "ImageMask 文件嵌入",
            PURPLE,
            vec![
                req("data", "载体图片", PortType::Any),
                req("file", "要嵌入的文件", PortType::Any),
            ],
            vec![
                req("image", "图片", PortType::Image),
                opt("bytes", "PNG字节", PortType::Bytes),
            ],
            {
                let mut v = vec![ParamSpec::text(
                    "filename",
                    "记录的文件名",
                    "secret.bin",
                    false,
                )];
                v.extend(mask_params());
                v
            },
        ),
        Arc::new(|| Arc::new(FileEmbed)),
    );
}
