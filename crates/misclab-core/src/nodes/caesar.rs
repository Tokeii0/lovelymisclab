//! Caesar / ROT-N shift cipher over the Latin alphabet.
use super::prelude::*;

struct N;
impl Node for N {
    fn run(
        &self,
        inputs: &PortMap,
        params: &serde_json::Value,
        _ctx: &mut NodeCtx,
    ) -> Result<PortMap, CoreError> {
        let n = (pnum(params, "amount", 3.0) as i64).rem_euclid(26) as u8;
        let s: String = in_text(inputs, "text")?
            .chars()
            .map(|c| match c {
                'a'..='z' => (((c as u8 - b'a' + n) % 26) + b'a') as char,
                'A'..='Z' => (((c as u8 - b'A' + n) % 26) + b'A') as char,
                o => o,
            })
            .collect();
        Ok(out_text(s))
    }
}

pub fn register(reg: &mut NodeRegistry) {
    reg.register(
        desc(
            "caesar",
            CRYPTO,
            "凯撒密码",
            ROSE,
            vec![t_in()],
            vec![t_out()],
            vec![ParamSpec::number("amount", "位移量", 0.0, 25.0, 1.0, 3.0)],
        ),
        Arc::new(|| Arc::new(N)),
    );
}
