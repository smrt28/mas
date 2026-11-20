//! A subprocess is essential: the sandbox is irreversible and closes inherited fds.
#![cfg(all(target_os = "linux", feature = "network-tests"))]
use std::{
    fs,
    io::{self, Read},
    net::TcpListener,
    os::fd::AsRawFd,
    process::{Command, Stdio},
};

#[test]
fn server_sandbox_denies_disk_access_in_current_existing_and_future_threads() {
    let directory = std::env::temp_dir().join(format!(
        "mas-sandbox-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    fs::create_dir(&directory).unwrap();
    let canary = directory.join("canary");
    let log = directory.join("redirected-stderr");
    fs::write(&canary, b"disk canary").unwrap();
    let inherited = fs::File::open(&canary).unwrap();
    let fd = inherited.as_raw_fd();
    // SAFETY: this test owns the fd. Make it inheritable by this subprocess only.
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "sandbox_probe", "--nocapture"])
        .env("MAS_SANDBOX_PROBE", &canary)
        .env("MAS_SANDBOX_INHERITED_FD", fd.to_string())
        .stdin(fs::File::open(&canary).unwrap())
        .stderr(fs::File::create(&log).unwrap())
        .stdout(Stdio::piped())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "Sandbox probe failed: {}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert_eq!(fs::read(&canary).unwrap(), b"disk canary");
    assert!(fs::read(&log).unwrap().is_empty());
    drop(inherited);
    fs::remove_dir_all(directory).unwrap();
}

fn denied<T: std::fmt::Debug>(result: io::Result<T>) {
    assert_eq!(result.unwrap_err().raw_os_error(), Some(libc::EPERM));
}
#[test]
fn sandbox_probe() {
    let Some(path) = std::env::var_os("MAS_SANDBOX_PROBE") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    let inherited: i32 = std::env::var("MAS_SANDBOX_INHERITED_FD")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"disk canary");
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    assert_ne!(listener.as_raw_fd(), inherited);
    let (tx, rx) = std::sync::mpsc::channel();
    let worker_path = path.clone();
    let existing = std::thread::spawn(move || {
        rx.recv().unwrap();
        denied(fs::read(worker_path));
    });
    marsal_a_spion::sandbox::enter(&listener).unwrap();
    // Inherited disk descriptors are closed, not just hidden behind path rules.
    // SAFETY: fcntl only queries a descriptor number; no memory is dereferenced.
    assert_eq!(unsafe { libc::fcntl(inherited, libc::F_GETFD) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
    denied(fs::read(&path));
    denied(fs::write(&path, b"modified"));
    denied(fs::File::create(path.with_extension("created")));
    denied(fs::metadata(&path));
    denied(fs::read_dir(path.parent().unwrap()));
    denied(fs::rename(&path, path.with_extension("renamed")));
    denied(fs::remove_file(&path));
    denied(std::net::TcpStream::connect(listener.local_addr().unwrap()));
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    assert!(input.is_empty());
    // SAFETY: these calls are denied before the kernel reads pointer arguments.
    // This also checks modern filesystem/async-I/O and process-launch bypasses.
    unsafe {
        for number in [
            libc::SYS_openat2,
            libc::SYS_io_uring_setup,
            libc::SYS_execve,
            libc::SYS_socket,
            libc::SYS_recvmsg,
        ] {
            assert_eq!(libc::syscall(number, 0, 0, 0, 0, 0, 0), -1);
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM));
        }
        assert_eq!(libc::syscall(libc::SYS_clone3, 0, 0), -1);
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ENOSYS)
        );
        let mapped = libc::mmap(
            std::ptr::null_mut(),
            4096,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            1,
            0,
        );
        assert_eq!(mapped, libc::MAP_FAILED);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM));
        let message = b"must not reach redirected disk log";
        assert_eq!(
            libc::write(2, message.as_ptr().cast(), message.len()),
            message.len() as isize
        );
    }
    tx.send(()).unwrap();
    existing.join().unwrap();
    std::thread::spawn(move || denied(fs::read(path)))
        .join()
        .unwrap();
    let mut memory = vec![0_u8; 1024 * 1024];
    memory.fill(42);
    assert_eq!(memory[100], 42);
    use rand::TryRngCore;
    rand::rngs::OsRng.try_next_u64().unwrap();
    println!("Sandbox filesystem and thread probes passed");
}
