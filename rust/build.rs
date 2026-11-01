use embed_manifest::{embed_manifest, new_manifest};

fn main() {
    // 嵌入清单：启用 Win10/11 原生控件样式和高 DPI 支持
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("SitReminder")).expect("unable to embed manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
