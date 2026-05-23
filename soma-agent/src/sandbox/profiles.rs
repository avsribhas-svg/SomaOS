//! Seccomp BPF filter builder — Linux only.
//!
//! Produces a compiled BPF program that allows a per-category syscall whitelist
//! and kills the calling process on any unlisted syscall.

#![cfg(target_os = "linux")]

use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, SeccompRule};
use std::collections::BTreeMap;

use super::CapabilityCategory;

/// Build a seccomp BPF filter for the given capability category.
/// Returns `None` if compilation fails (should not happen with valid inputs).
pub fn build_seccomp_filter(cat: CapabilityCategory) -> Option<BpfProgram> {
    let syscalls = allowed_syscalls(cat);
    let rules: BTreeMap<i64, Vec<SeccompRule>> = syscalls
        .iter()
        .map(|&sc| (sc, vec![]))
        .collect();

    SeccompFilter::new(
        rules,
        SeccompAction::KillProcess,
        SeccompAction::Allow,
        std::env::consts::ARCH.try_into().ok()?,
    )
    .ok()?
    .try_into()
    .ok()
}

fn allowed_syscalls(cat: CapabilityCategory) -> &'static [i64] {
    match cat {
        CapabilityCategory::Filesystem => &[
            libc::SYS_read, libc::SYS_write, libc::SYS_open, libc::SYS_openat,
            libc::SYS_close, libc::SYS_stat, libc::SYS_fstat, libc::SYS_lstat,
            libc::SYS_unlink, libc::SYS_rename, libc::SYS_mkdir, libc::SYS_rmdir,
            libc::SYS_getdents64, libc::SYS_lseek,
            libc::SYS_exit_group, libc::SYS_brk, libc::SYS_mmap,
            libc::SYS_munmap, libc::SYS_mprotect, libc::SYS_rt_sigreturn,
            libc::SYS_pipe2,
        ],
        CapabilityCategory::Network => &[
            libc::SYS_socket, libc::SYS_connect, libc::SYS_bind, libc::SYS_listen,
            libc::SYS_accept4, libc::SYS_sendto, libc::SYS_recvfrom,
            libc::SYS_read, libc::SYS_write, libc::SYS_close,
            libc::SYS_poll, libc::SYS_epoll_wait, libc::SYS_epoll_ctl, libc::SYS_epoll_create1,
            libc::SYS_fcntl,
            libc::SYS_exit_group, libc::SYS_brk, libc::SYS_mmap,
            libc::SYS_munmap, libc::SYS_mprotect, libc::SYS_rt_sigreturn,
            libc::SYS_pipe2,
        ],
        CapabilityCategory::Process => &[
            libc::SYS_fork, libc::SYS_clone, libc::SYS_execve, libc::SYS_execveat,
            libc::SYS_waitpid, libc::SYS_wait4, libc::SYS_kill,
            libc::SYS_read, libc::SYS_write, libc::SYS_close, libc::SYS_pipe2,
            libc::SYS_exit_group, libc::SYS_brk, libc::SYS_mmap,
            libc::SYS_munmap, libc::SYS_mprotect, libc::SYS_rt_sigreturn,
        ],
        CapabilityCategory::Media => &[
            libc::SYS_socket, libc::SYS_connect,
            libc::SYS_read, libc::SYS_write, libc::SYS_close, libc::SYS_poll,
            libc::SYS_exit_group, libc::SYS_brk, libc::SYS_mmap,
            libc::SYS_munmap, libc::SYS_mprotect, libc::SYS_rt_sigreturn,
            libc::SYS_pipe2,
        ],
        CapabilityCategory::Meta => &[
            libc::SYS_open, libc::SYS_openat, libc::SYS_read, libc::SYS_write,
            libc::SYS_close, libc::SYS_stat, libc::SYS_fstat, libc::SYS_getdents64,
            libc::SYS_lseek,
            libc::SYS_exit_group, libc::SYS_brk, libc::SYS_mmap,
            libc::SYS_munmap, libc::SYS_mprotect, libc::SYS_rt_sigreturn,
            libc::SYS_pipe2,
        ],
        CapabilityCategory::Script | CapabilityCategory::Wasm => &[
            libc::SYS_fork, libc::SYS_clone, libc::SYS_execve,
            libc::SYS_read, libc::SYS_write, libc::SYS_open, libc::SYS_openat,
            libc::SYS_close, libc::SYS_socket, libc::SYS_connect,
            libc::SYS_sendto, libc::SYS_recvfrom, libc::SYS_poll,
            libc::SYS_stat, libc::SYS_fstat,
            libc::SYS_exit_group, libc::SYS_brk, libc::SYS_mmap,
            libc::SYS_munmap, libc::SYS_mprotect, libc::SYS_rt_sigreturn,
            libc::SYS_pipe2, libc::SYS_fcntl,
        ],
    }
}
