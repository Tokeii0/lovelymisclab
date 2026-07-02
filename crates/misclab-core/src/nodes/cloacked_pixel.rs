//! cloacked-pixel（livz/cloacked-pixel）LSB + AES 隐写。
//!
//! 载荷先 AES-256-CBC 加密：`key = SHA256(password)`，随机 16 字节 IV，明文按块大小
//! 32 做 PKCS#7 式填充；密文 = `IV(16) || AES-CBC(padded)`。再前置 4 字节小端长度
//! （`struct.pack("i", len)`），整体拆成比特（**高位在前**），依次写入每个像素
//! R、G、B 通道的最低位（跳过 Alpha），行主序。提取即逆过程。
use aes::cipher::block_padding::NoPadding;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use sha2::{Digest, Sha256};

use super::image_util::{image_out, load_image};
use super::prelude::*;

type Enc = cbc::Encryptor<aes::Aes256>;
type Dec = cbc::Decryptor<aes::Aes256>;

/// 把比特（高位在前）打包成字节。
fn pack_msb(bits: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bits.len() / 8);
    for chunk in bits.chunks(8) {
        if chunk.len() < 8 {
            break;
        }
        let mut b = 0u8;
        for &bit in chunk {
            b = (b << 1) | (bit & 1);
        }
        out.push(b);
    }
    out
}

/// cloacked-pixel 的按块 32 填充（值 = 32-len%32，1..=32）。
fn pad32(mut v: Vec<u8>) -> Vec<u8> {
    let pad = 32 - v.len() % 32;
    v.extend(std::iter::repeat_n(pad as u8, pad));
    v
}

fn unpad32(v: &[u8]) -> Result<Vec<u8>, CoreError> {
    let n = *v.last().ok_or_else(|| CoreError::Parse("空明文".into()))? as usize;
    if n == 0 || n > 32 || n > v.len() {
        return Err(CoreError::Parse("填充无效（密码错误？）".into()));
    }
    Ok(v[..v.len() - n].to_vec())
}

// ---------------------------------------------------------------- 提取
struct Extract;
impl Node for Extract {
    fn run(
        &self,
        i: &PortMap,
        p: &serde_json::Value,
        _c: &mut NodeCtx,
    ) -> Result<PortMap, CoreError> {
        let img = load_image(i, "data")?;
        // 每像素取 R、G、B 最低位（行主序，跳过 Alpha）。
        let mut bits = Vec::with_capacity(img.pixels().len() * 3);
        for px in img.pixels() {
            bits.push(px.0[0] & 1);
            bits.push(px.0[1] & 1);
            bits.push(px.0[2] & 1);
        }
        let raw = pack_msb(&bits);
        if raw.len() < 4 {
            return Err(CoreError::Parse("图片过小，无法提取。".into()));
        }
        let n = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
        if n < 16 || 4 + n > raw.len() {
            return Err(CoreError::Parse(
                "长度字段无效：可能不是 cloacked-pixel 隐写图。".into(),
            ));
        }
        let enc = &raw[4..4 + n];
        let (iv, ct) = enc.split_at(16);
        if ct.is_empty() || ct.len() % 16 != 0 {
            return Err(CoreError::Parse("密文长度无效。".into()));
        }

        let key = Sha256::digest(pstr(p, "password", "").as_bytes());
        let mut buf = ct.to_vec();
        let pt = Dec::new_from_slices(&key, iv)
            .map_err(|_| CoreError::Parse("密钥/IV 长度错误".into()))?
            .decrypt_padded_mut::<NoPadding>(&mut buf)
            .map_err(|_| CoreError::Parse("解密失败。".into()))?;
        let data = unpad32(pt)?;

        let mut m = PortMap::new();
        m.insert(
            "text".into(),
            PortValue::Text(String::from_utf8_lossy(&data).into_owned()),
        );
        m.insert(
            "bytes".into(),
            PortValue::Bytes(Arc::from(data.clone().into_boxed_slice())),
        );
        m.insert("hex".into(), PortValue::Text(hex::encode(&data)));
        Ok(m)
    }
}

// ---------------------------------------------------------------- 嵌入
struct Embed;
impl Node for Embed {
    fn run(
        &self,
        i: &PortMap,
        p: &serde_json::Value,
        _c: &mut NodeCtx,
    ) -> Result<PortMap, CoreError> {
        let mut img = load_image(i, "data")?;
        let payload = in_bytes(i, "file")?;
        let key = Sha256::digest(pstr(p, "password", "").as_bytes());

        // 随机 IV。
        let mut iv = [0u8; 16];
        getrandom::getrandom(&mut iv).map_err(|e| CoreError::Other(format!("随机数失败: {e}")))?;
        let ct = Enc::new_from_slices(&key, &iv)
            .map_err(|_| CoreError::Parse("密钥长度错误".into()))?
            .encrypt_padded_vec_mut::<NoPadding>(&pad32(payload));
        let mut enc = iv.to_vec();
        enc.extend_from_slice(&ct);

        // 4 字节小端长度 + enc → 比特（高位在前）。
        let mut bytes = (enc.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&enc);
        let mut bits = Vec::with_capacity(bytes.len() * 8);
        for b in bytes {
            for k in (0..8).rev() {
                bits.push((b >> k) & 1);
            }
        }
        let cap = img.pixels().len() * 3;
        if bits.len() > cap {
            return Err(CoreError::Other(format!(
                "图片容量不足：需 {} 位，仅 {cap} 位。",
                bits.len()
            )));
        }
        let mut idx = 0usize;
        'outer: for px in img.pixels_mut() {
            for ch in 0..3 {
                if idx >= bits.len() {
                    break 'outer;
                }
                px.0[ch] = (px.0[ch] & !1) | bits[idx];
                idx += 1;
            }
        }
        image_out(&img)
    }
}

pub fn register(reg: &mut NodeRegistry) {
    reg.register(
        desc(
            "cloacked_pixel_extract",
            STEG,
            "cloacked-pixel 提取",
            PURPLE,
            vec![req("data", "图片", PortType::Any)],
            vec![
                req("text", "文本", PortType::Text),
                opt("bytes", "字节", PortType::Bytes),
                opt("hex", "hex", PortType::Text),
            ],
            vec![ParamSpec::text("password", "密码", "", false)],
        ),
        Arc::new(|| Arc::new(Extract)),
    );
    reg.register(
        desc(
            "cloacked_pixel_embed",
            STEG,
            "cloacked-pixel 嵌入",
            PURPLE,
            vec![
                req("data", "载体图片", PortType::Any),
                req("file", "载荷", PortType::Any),
            ],
            vec![
                req("image", "图片", PortType::Image),
                opt("bytes", "PNG字节", PortType::Bytes),
            ],
            vec![ParamSpec::text("password", "密码", "", false)],
        ),
        Arc::new(|| Arc::new(Embed)),
    );
}
