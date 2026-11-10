pub fn save(path: &str, data: &str) {
    match std::fs::write(path, data) {
        Ok(()) => {}
        Err(_) => {}
    }
}

pub fn read(path: &str) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

pub fn bump(counter: &mut u32) {
    // increment counter
    *counter += 1;
}

pub fn drop_err(path: &str) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(_) => { /* already gone */ }
    }
}
