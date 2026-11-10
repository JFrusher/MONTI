pub fn save(path: &str, data: &str) -> std::io::Result<()> {
    std::fs::write(path, data).map_err(|e| {
        eprintln!("cannot save {path}");
        e
    })
}

pub fn bump(counter: &mut u32) {
    // saturating: the counter is shown to users and must never wrap to 0
    *counter = counter.saturating_add(1);
}

fn label(name: &str, v: f64) -> String {
    format!("{name}: {v}")
}
