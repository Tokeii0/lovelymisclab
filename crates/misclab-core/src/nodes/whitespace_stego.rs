//! Whitespace steganography (SNOW-style) — hide a bitstream in spaces and tabs.
//! Each secret bit becomes a space or a tab; the run is appended as trailing
//! whitespace so it stays invisible in most editors. Decode reads the trailing
//! whitespace of every line (or every space/tab in the text) back into bytes.
//! A staple of CTF misc "there's something after the line" challenges.
use super::prelude::*;

const SP: char = ' ';
const TAB: char = '\t';

/// Resolve the (zero, one) whitespace characters from the "0=" param.
fn ws_chars(zero_label: &str) -> (char, char) {
    if zero_label.starts_with("制表符") {
        (TAB, SP)
    } else {
        (SP, TAB)
    }
}

fn bytes_to_bits(bytes: &[u8], msb: bool) -> String {
    let mut s = String::with_capacity(bytes.len() * 8);
    for &b in bytes {
        for i in 0..8 {
            let shift = if msb { 7 - i } else { i };
            s.push(if (b >> shift) & 1 == 1 { '1' } else { '0' });
        }
    }
    s
}

fn bits_to_bytes(bits: &str, msb: bool) -> Vec<u8> {
    let flags: Vec<u8> = bits.bytes().filter(|&c| c == b'0' || c == b'1').collect();
    let mut out = Vec::with_capacity(flags.len() / 8);
    for chunk in flags.chunks(8) {
        if chunk.len() < 8 {
            break;
        }
        let mut byte = 0u8;
        for (i, &c) in chunk.iter().enumerate() {
            if c == b'1' {
                byte |= 1 << if msb { 7 - i } else { i };
            }
        }
        out.push(byte);
    }
    out
}

/// The maximal run of space/tab at the end of a line (after stripping a CR).
fn trailing_ws(line: &str) -> String {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let rev: String = line
        .chars()
        .rev()
        .take_while(|&c| c == SP || c == TAB)
        .collect();
    rev.chars().rev().collect()
}

struct Encode;
impl Node for Encode {
    fn run(
        &self,
        inputs: &PortMap,
        params: &serde_json::Value,
        _ctx: &mut NodeCtx,
    ) -> Result<PortMap, CoreError> {
        let secret = in_text(inputs, "text")?;
        let (zero_c, one_c) = ws_chars(pstr(params, "zero", "空格 (space)"));
        let msb = pbool(params, "msb", true);
        let cover = pstr(params, "cover", "");

        let hidden: String = bytes_to_bits(secret.as_bytes(), msb)
            .chars()
            .map(|b| if b == '1' { one_c } else { zero_c })
            .collect();
        // Append as trailing whitespace of the (last) line.
        let result = format!("{cover}{hidden}");
        Ok(out_text(result))
    }
}

struct Decode;
impl Node for Decode {
    fn run(
        &self,
        inputs: &PortMap,
        params: &serde_json::Value,
        _ctx: &mut NodeCtx,
    ) -> Result<PortMap, CoreError> {
        let input = in_text(inputs, "text")?;
        let (zero_c, one_c) = ws_chars(pstr(params, "zero", "空格 (space)"));
        let msb = pbool(params, "msb", true);
        let all = pstr(params, "scope", "行尾") == "全部";

        // Collect the carrier whitespace: every trailing run, or every space/tab.
        let ws: String = if all {
            input.chars().filter(|&c| c == SP || c == TAB).collect()
        } else {
            input.split('\n').map(trailing_ws).collect()
        };

        let bits: String = ws
            .chars()
            .filter_map(|c| {
                if c == zero_c {
                    Some('0')
                } else if c == one_c {
                    Some('1')
                } else {
                    None
                }
            })
            .collect();
        let bytes = bits_to_bytes(&bits, msb);
        let text = String::from_utf8_lossy(&bytes).into_owned();

        let (spaces, tabs) = (
            ws.chars().filter(|&c| c == SP).count(),
            ws.chars().filter(|&c| c == TAB).count(),
        );
        let report = if bits.is_empty() {
            format!("未发现可解码的空白（空格 {spaces}，制表符 {tabs}）。")
        } else {
            format!(
                "空白字符：空格 {spaces}，制表符 {tabs}；0={} 1={}（{}），共 {} 位 → {} 字节。",
                if zero_c == SP { "空格" } else { "制表符" },
                if one_c == TAB { "制表符" } else { "空格" },
                if msb { "MSB" } else { "LSB" },
                bits.len(),
                bytes.len()
            )
        };

        let mut m = PortMap::new();
        m.insert("text".into(), PortValue::Text(text));
        m.insert("bits".into(), PortValue::Text(bits));
        m.insert("report".into(), PortValue::Text(report));
        Ok(m)
    }
}

pub fn register(reg: &mut NodeRegistry) {
    let zero_opts = &["空格 (space)", "制表符 (tab)"];
    reg.register(
        desc(
            "whitespace_encode",
            STEG,
            "空白隐写编码",
            CYAN,
            vec![req("text", "秘密信息", PortType::Text)],
            vec![req("text", "结果", PortType::Text)],
            vec![
                ParamSpec::text("cover", "载体文本", "", false),
                ParamSpec::select("zero", "0 = 字符", zero_opts, "空格 (space)"),
                ParamSpec::toggle("msb", "高位在前 (MSB)", true),
            ],
        ),
        Arc::new(|| Arc::new(Encode)),
    );
    reg.register(
        desc(
            "whitespace_decode",
            STEG,
            "空白隐写解码",
            CYAN,
            vec![req("text", "载体文本", PortType::Text)],
            vec![
                req("text", "结果", PortType::Text),
                opt("bits", "位串", PortType::Text),
                opt("report", "分析", PortType::Text),
            ],
            vec![
                ParamSpec::select("zero", "0 = 字符", zero_opts, "空格 (space)"),
                ParamSpec::select("scope", "范围", &["行尾", "全部"], "行尾"),
                ParamSpec::toggle("msb", "高位在前 (MSB)", true),
            ],
        ),
        Arc::new(|| Arc::new(Decode)),
    );
}
