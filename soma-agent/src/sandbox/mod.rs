pub mod profiles;

/// Broad categories of capability work (used for both seccomp and AppArmor profiling).
/// Defined unconditionally so callers compile on all platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityCategory {
    Filesystem,
    Network,
    Process,
    Media,
    Meta,
    Script,
    Wasm,
}

/// Map a capability name to its sandbox category.
pub fn category_for(capability: &str) -> CapabilityCategory {
    match capability {
        "filesystem" | "semantic_fs" | "docs" | "sheets" => CapabilityCategory::Filesystem,
        "network" | "browser"                            => CapabilityCategory::Network,
        "process" | "system" | "package"                => CapabilityCategory::Process,
        "media"                                          => CapabilityCategory::Media,
        "meta" | "desktop_agent" | "delegate"           => CapabilityCategory::Meta,
        "script"                                         => CapabilityCategory::Script,
        _                                                => CapabilityCategory::Wasm,
    }
}

/// Returns true if the current platform supports seccomp sandboxing.
pub fn is_supported() -> bool {
    cfg!(target_os = "linux")
}

/// Execute `f` inside a seccomp-filtered child process and return the result
/// via a pipe. Falls back to direct execution on non-Linux or if fork fails.
#[cfg(target_os = "linux")]
pub fn execute_sandboxed<F>(
    category: CapabilityCategory,
    f: F,
) -> soma_common::CapabilityResult
where
    F: FnOnce() -> soma_common::CapabilityResult,
{
    use nix::unistd::{fork, ForkResult};
    use std::io::{Read, Write};
    use std::os::unix::io::FromRawFd;

    let mut fds = [0i32; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return f(); // pipe creation failed — fall back
    }
    let (read_fd, write_fd) = (fds[0], fds[1]);

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            if let Some(filter) = profiles::build_seccomp_filter(category) {
                let _ = seccompiler::apply_filter(&filter);
            }
            let result = f();
            let json = serde_json::to_string(&result).unwrap_or_default();
            let mut pipe_writer = unsafe { std::fs::File::from_raw_fd(write_fd) };
            unsafe { libc::close(read_fd) };
            let _ = pipe_writer.write_all(json.as_bytes());
            std::process::exit(0);
        }
        Ok(ForkResult::Parent { child }) => {
            unsafe { libc::close(write_fd) };
            let mut pipe_reader = unsafe { std::fs::File::from_raw_fd(read_fd) };
            let mut buf = String::new();
            let _ = pipe_reader.read_to_string(&mut buf);
            let _ = nix::sys::wait::waitpid(child, None);
            serde_json::from_str::<soma_common::CapabilityResult>(&buf)
                .unwrap_or(soma_common::CapabilityResult {
                    success: false,
                    data: serde_json::Value::Null,
                    error: Some(soma_common::CapabilityError::new(
                        soma_common::ErrorReason::InternalError,
                        "Sandboxed child produced no result".to_string(),
                    )),
                    state_delta: None,
})
        }
        Err(_) => f(),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn execute_sandboxed<F>(
    _category: CapabilityCategory,
    f: F,
) -> soma_common::CapabilityResult
where
    F: FnOnce() -> soma_common::CapabilityResult,
{
    f()
}
