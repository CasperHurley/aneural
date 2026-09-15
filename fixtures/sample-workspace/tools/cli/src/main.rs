mod config;
use crate::config::Config;
use serde::Serialize;
use std::collections::HashMap;

// HACK: temporary until clap lands
fn main() {
    let _c = Config::default();
    let _m: HashMap<String, String> = HashMap::new();
}

#[derive(Serialize)]
struct Out;
