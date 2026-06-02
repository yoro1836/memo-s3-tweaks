use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub struct Config {
    pub vid: u16,
    pub pid: u16,
    pub plugin_dir: String,
    pub values: HashMap<String, String>,
}

impl Config {
    pub fn load(path: &Path) -> Self {
        let mut cfg = Config {
            vid: 0x045e, pid: 0x028e,
            plugin_dir: "/data/user_de/0/com.android.shell/axeron/plugins/keyforge/plugins".into(),
            values: HashMap::new(),
        };
        if let Ok(file) = fs::File::open(path) {
            for line in BufReader::new(file).lines().map_while(Result::ok) {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') { continue; }
                if let Some((k, v)) = trimmed.split_once('=') {
                    let key = k.trim().to_lowercase();
                    let val = v.trim().to_string();
                    match key.as_str() {
                        "vid" => cfg.vid = parse_hex16(&val),
                        "pid" => cfg.pid = parse_hex16(&val),
                        "plugin_dir" => cfg.plugin_dir = val,
                        _ => { cfg.values.insert(key, val); }
                    }
                }
            }
        }
        cfg
    }
}

fn parse_hex16(s: &str) -> u16 {
    let s = s.trim().strip_prefix("0x").unwrap_or(s);
    u16::from_str_radix(s, 16).unwrap_or(0)
}
