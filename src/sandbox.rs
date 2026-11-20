//! Server-only, irreversible removal of filesystem and process-launch access.
use std::{io, net::TcpListener};

/// Call after loading configuration/binding, but before accepting connections.
/// Unsupported platforms fail closed instead of serving without a sandbox.
pub fn enter(listener: &TcpListener) -> io::Result<()> {
    #[cfg(all(
        target_os = "linux",
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "riscv64"
        )
    ))]
    return linux::enter(listener);
    #[cfg(not(all(
        target_os = "linux",
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "riscv64"
        )
    )))]
    {
        let _ = listener;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Server sandbox requires supported 64-bit Linux",
        ))
    }
}

#[cfg(all(
    target_os = "linux",
    any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64"
    )
))]
mod linux {
    use super::*;
    use seccompiler::{
        BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
        SeccompRule,
    };
    use std::{collections::BTreeMap, fs::OpenOptions, os::fd::AsRawFd};

    fn rule(index: u8, op: SeccompCmpOp, value: u64) -> io::Result<SeccompRule> {
        let condition = SeccompCondition::new(index, SeccompCmpArgLen::Qword, op, value)
            .map_err(io::Error::other)?;
        SeccompRule::new(vec![condition]).map_err(io::Error::other)
    }
    fn compile(
        rules: BTreeMap<i64, Vec<SeccompRule>>,
        default: SeccompAction,
        matched: SeccompAction,
    ) -> io::Result<BpfProgram> {
        let arch = std::env::consts::ARCH
            .try_into()
            .map_err(io::Error::other)?;
        SeccompFilter::new(rules, default, matched, arch)
            .map_err(io::Error::other)?
            .try_into()
            .map_err(io::Error::other)
    }
    fn policy() -> io::Result<BpfProgram> {
        // No file opens, path metadata, directory enumeration, filesystem writes,
        // exec, ptrace, io_uring, new sockets, or descriptor receipt via recvmsg.
        let mut rules: BTreeMap<_, _> = [
            libc::SYS_read,
            libc::SYS_write,
            libc::SYS_readv,
            libc::SYS_writev,
            libc::SYS_close,
            libc::SYS_fstat,
            libc::SYS_accept4,
            libc::SYS_recvfrom,
            libc::SYS_sendto,
            libc::SYS_shutdown,
            libc::SYS_getsockname,
            libc::SYS_getpeername,
            libc::SYS_getsockopt,
            libc::SYS_setsockopt,
            libc::SYS_ppoll,
            libc::SYS_pselect6,
            libc::SYS_brk,
            libc::SYS_mprotect,
            libc::SYS_munmap,
            libc::SYS_mremap,
            libc::SYS_madvise,
            libc::SYS_futex,
            libc::SYS_set_robust_list,
            libc::SYS_set_tid_address,
            libc::SYS_rseq,
            libc::SYS_sched_yield,
            libc::SYS_sched_getaffinity,
            libc::SYS_clock_gettime,
            libc::SYS_clock_nanosleep,
            libc::SYS_nanosleep,
            libc::SYS_getrandom,
            libc::SYS_getpid,
            libc::SYS_gettid,
            libc::SYS_rt_sigaction,
            libc::SYS_rt_sigprocmask,
            libc::SYS_rt_sigreturn,
            libc::SYS_sigaltstack,
            libc::SYS_tgkill,
            libc::SYS_exit,
            libc::SYS_exit_group,
            // The separate filter below always returns ENOSYS for clone3,
            // forcing libc to use clone with inspectable thread-only flags.
            libc::SYS_clone3,
        ]
        .into_iter()
        .map(|number| (number, vec![]))
        .collect();
        #[cfg(target_arch = "x86_64")]
        for number in [libc::SYS_accept, libc::SYS_poll, libc::SYS_getrlimit] {
            rules.insert(number, vec![]);
        }
        let thread_flags =
            (libc::CLONE_VM | libc::CLONE_SIGHAND | libc::CLONE_THREAD | libc::CLONE_FILES) as u64;
        rules.insert(
            libc::SYS_clone,
            vec![rule(0, SeccompCmpOp::MaskedEq(thread_flags), thread_flags)?],
        );
        // Anonymous memory only: an inherited file cannot be mapped back in.
        rules.insert(
            libc::SYS_mmap,
            vec![rule(
                3,
                SeccompCmpOp::MaskedEq(libc::MAP_ANONYMOUS as u64),
                libc::MAP_ANONYMOUS as u64,
            )?],
        );
        rules.insert(libc::SYS_prlimit64, vec![rule(2, SeccompCmpOp::Eq, 0)?]);
        rules.insert(
            libc::SYS_fcntl,
            [
                libc::F_DUPFD_CLOEXEC,
                libc::F_GETFD,
                libc::F_SETFD,
                libc::F_GETFL,
                libc::F_SETFL,
            ]
            .into_iter()
            .map(|command| rule(1, SeccompCmpOp::Eq, command as u64))
            .collect::<io::Result<_>>()?,
        );
        compile(
            rules,
            SeccompAction::Errno(libc::EPERM as u32),
            SeccompAction::Allow,
        )
    }
    fn discard_file_descriptors(listener: &TcpListener) -> io::Result<()> {
        let listener_fd = listener.as_raw_fd();
        if listener_fd < 3 {
            return Err(io::Error::other(
                "Listener must not use a standard descriptor",
            ));
        }
        let null = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/null")?;
        for fd in 0..=2 {
            // Preserve terminal/pipe/socket logging, never a pre-opened disk file.
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            // SAFETY: stat points to writable storage; fd is a standard descriptor.
            let keep = fd != 0
                && unsafe {
                    libc::fstat(fd, stat.as_mut_ptr()) == 0 && {
                        let mode = stat.assume_init().st_mode & libc::S_IFMT;
                        mode == libc::S_IFIFO || mode == libc::S_IFSOCK || libc::isatty(fd) == 1
                    }
                };
            // SAFETY: null is live; dup2 atomically replaces only fd 0, 1 or 2.
            if !keep && unsafe { libc::dup2(null.as_raw_fd(), fd) } == -1 {
                return Err(io::Error::last_os_error());
            }
        }
        drop(null);
        let close_range = |first: u32, last: u32| -> io::Result<()> {
            // SAFETY: closes only inherited descriptors, preserving the listener
            // and standard streams. Must run before accepting player sockets.
            if first <= last
                && unsafe { libc::syscall(libc::SYS_close_range, first, last, 0_u32) } != 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        };
        close_range(3, listener_fd as u32 - 1)?;
        close_range(listener_fd as u32 + 1, u32::MAX)
    }
    pub(super) fn enter(listener: &TcpListener) -> io::Result<()> {
        // Build both filters before irreversibly changing process permissions.
        let runtime = policy()?;
        let clone3 = compile(
            [(libc::SYS_clone3, vec![])].into(),
            SeccompAction::Allow,
            SeccompAction::Errno(libc::ENOSYS as u32),
        )?;
        discard_file_descriptors(listener)?;
        // The library sets no_new_privs and applies each filter with TSYNC.
        // All current threads are synchronized; future workers inherit both.
        seccompiler::apply_filter_all_threads(&clone3).map_err(io::Error::other)?;
        seccompiler::apply_filter_all_threads(&runtime).map_err(io::Error::other)
    }
}
