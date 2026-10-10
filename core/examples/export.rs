//! `export <portable-root> <meeting-id> <out-dir>`: Markdown + DOCX the way the app writes them.
use lariksa_core::{paths::Paths, service::LariksaService};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let svc = LariksaService::new(Paths::new(a[1].clone().into())).unwrap();
    let out = std::path::PathBuf::from(&a[3]);
    println!("{}", svc.export_markdown(&a[2], &out.join("rs-export.md")));
    println!("{}", svc.export_docx(&a[2], &out.join("rs-export.docx")));
    svc.shutdown();
}
