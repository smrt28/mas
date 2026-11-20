use std::{env, fs, path::PathBuf};
fn main() {
    println!("cargo:rerun-if-env-changed=MAS_SHARED_KEY_FILE");
    let path = env::var_os("MAS_SHARED_KEY_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".secrets/multiplayer.key"));
    println!("cargo:rerun-if-changed={}", path.display());
    let key = fs::read(path).expect("Missing shared build key: run python3 tools/generate_shared_key.py or set MAS_SHARED_KEY_FILE");
    assert_eq!(
        key.len(),
        32,
        "Shared build key must contain exactly 32 raw bytes"
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("shared-key.bin");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    options
        .open(output)
        .expect("Cannot create private build-key artifact")
        .write_all(&key)
        .expect("Cannot embed shared build key");
}
