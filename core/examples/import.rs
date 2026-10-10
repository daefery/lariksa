//! `import <portable-root> <file>...`: import like the app's dialog does and wait for the notulen.
use lariksa_core::{paths::Paths, service::LariksaService};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let svc = LariksaService::new(Paths::new(a[1].clone().into())).unwrap();
    for f in &a[2..] {
        let t = std::time::Instant::now();
        let r = svc.import_file(std::path::Path::new(f), "");
        let id = r["meeting_id"].as_str().unwrap_or_default().to_string();
        loop {
            let m = svc.store.get(&id).unwrap();
            if m.status == "done" || m.status == "failed" {
                let n = m.notulen.as_ref();
                println!("{f}: {} in {:.0}s, {} utterances, title={:?}, actions={}, err={:?}",
                    m.status, t.elapsed().as_secs_f64(), m.transcript.len(), m.title, n.map_or(0, |n| n.actions.len()), m.error);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
    svc.shutdown();
}
