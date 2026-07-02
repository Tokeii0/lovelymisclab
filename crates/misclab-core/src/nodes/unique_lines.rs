//! Deduplicate lines (preserve order), or count occurrences.
use std::collections::HashMap;

use super::prelude::*;

struct N;
impl Node for N {
    fn run(
        &self,
        inputs: &PortMap,
        params: &serde_json::Value,
        _ctx: &mut NodeCtx,
    ) -> Result<PortMap, CoreError> {
        let text = in_text(inputs, "text")?;
        if pbool(params, "count", false) {
            let mut counts: Vec<(String, usize)> = Vec::new();
            let mut idx: HashMap<&str, usize> = HashMap::new();
            for l in text.lines() {
                if let Some(&i) = idx.get(l) {
                    counts[i].1 += 1;
                } else {
                    idx.insert(l, counts.len());
                    counts.push((l.to_string(), 1));
                }
            }
            counts.sort_by(|a, b| b.1.cmp(&a.1));
            let out = counts
                .iter()
                .map(|(l, c)| format!("{c}\t{l}"))
                .collect::<Vec<_>>()
                .join("\n");
            Ok(out_text(out))
        } else {
            let mut seen = std::collections::HashSet::new();
            let out: Vec<&str> = text.lines().filter(|l| seen.insert(*l)).collect();
            Ok(out_text(out.join("\n")))
        }
    }
}

pub fn register(reg: &mut NodeRegistry) {
    reg.register(
        desc(
            "unique_lines",
            TXT,
            "行去重",
            TEAL,
            vec![t_in()],
            vec![t_out()],
            vec![ParamSpec::toggle("count", "统计出现次数", false)],
        ),
        Arc::new(|| Arc::new(N)),
    );
}
