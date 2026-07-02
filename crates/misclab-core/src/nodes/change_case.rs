//! Case transforms — upper/lower/title/sentence/swap.
use super::prelude::*;

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn sentence_case(s: &str) -> String {
    let mut out = String::new();
    let mut cap = true;
    for c in s.chars() {
        if cap && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            cap = false;
        } else {
            out.extend(c.to_lowercase());
            if matches!(c, '.' | '!' | '?') {
                cap = true;
            }
        }
    }
    out
}

struct N;
impl Node for N {
    fn run(
        &self,
        inputs: &PortMap,
        params: &serde_json::Value,
        _ctx: &mut NodeCtx,
    ) -> Result<PortMap, CoreError> {
        let s = in_text(inputs, "text")?;
        let out = match pstr(params, "mode", "大写") {
            "小写" => s.to_lowercase(),
            "词首大写" => title_case(s),
            "句首大写" => sentence_case(s),
            "交换大小写" => {
                let mut o = String::new();
                for c in s.chars() {
                    if c.is_uppercase() {
                        o.extend(c.to_lowercase());
                    } else if c.is_lowercase() {
                        o.extend(c.to_uppercase());
                    } else {
                        o.push(c);
                    }
                }
                o
            }
            _ => s.to_uppercase(),
        };
        Ok(out_text(out))
    }
}

pub fn register(reg: &mut NodeRegistry) {
    reg.register(
        desc(
            "change_case",
            TXT,
            "大小写转换",
            TEAL,
            vec![t_in()],
            vec![t_out()],
            vec![ParamSpec::select(
                "mode",
                "模式",
                &["大写", "小写", "词首大写", "句首大写", "交换大小写"],
                "大写",
            )],
        ),
        Arc::new(|| Arc::new(N)),
    );
}
