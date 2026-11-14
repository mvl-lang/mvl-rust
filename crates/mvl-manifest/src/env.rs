//! Runtime environment, collected at startup (ADR-0013 record 5).
//!
//! The loaded-library list covers dependencies Cargo never sees: glibc,
//! OpenSSL found via pkg-config, libSystem.

use crate::time::rfc3339_utc;
use crate::types::RuntimeEnv;

impl RuntimeEnv {
    /// Collect facts about the current process.
    pub fn collect() -> RuntimeEnv {
        let started_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| rfc3339_utc(d.as_secs() as i64))
            .unwrap_or_default();
        let exe = std::env::current_exe().ok();
        RuntimeEnv {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            os_version: os_version(),
            pid: std::process::id(),
            started_at,
            exe_path: exe
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            loaded_libraries: loaded_libraries(),
            libc_version: libc_version(),
            exe_digest: exe.as_deref().and_then(exe_digest),
        }
    }
}

#[cfg(feature = "exe-digest")]
fn exe_digest(path: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).ok()?;
    Some(format!("{:x}", h.finalize()))
}

#[cfg(not(feature = "exe-digest"))]
fn exe_digest(_path: &std::path::Path) -> Option<String> {
    None
}

#[cfg(unix)]
fn os_version() -> String {
    // SAFETY: `uname` fills a caller-owned, zero-initialised struct whose
    // fields are NUL-terminated C strings on success.
    unsafe {
        let mut u: libc::utsname = std::mem::zeroed();
        if libc::uname(&mut u) != 0 {
            return String::new();
        }
        let s = |f: &[libc::c_char]| {
            std::ffi::CStr::from_ptr(f.as_ptr())
                .to_string_lossy()
                .into_owned()
        };
        format!("{} {}", s(&u.sysname), s(&u.release))
    }
}

#[cfg(not(unix))]
fn os_version() -> String {
    String::new()
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn libc_version() -> Option<String> {
    // SAFETY: returns a pointer to a static NUL-terminated string.
    unsafe {
        let p = libc::gnu_get_libc_version();
        (!p.is_null()).then(|| std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned())
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn libc_version() -> Option<String> {
    None
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn loaded_libraries() -> Vec<String> {
    unsafe extern "C" fn callback(
        info: *mut libc::dl_phdr_info,
        _size: libc::size_t,
        data: *mut libc::c_void,
    ) -> libc::c_int {
        // SAFETY: `data` is the `&mut Vec<String>` passed below, alive for the
        // whole `dl_iterate_phdr` call; `info` is valid for this callback.
        // Every entry is kept, in order, so the filter can drop the first.
        let out = &mut *(data as *mut Vec<String>);
        let name = (*info).dlpi_name;
        out.push(if name.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(name)
                .to_string_lossy()
                .into_owned()
        });
        0
    }
    let mut out: Vec<String> = Vec::new();
    // SAFETY: the callback only touches `out` through the pointer we pass.
    unsafe {
        libc::dl_iterate_phdr(
            Some(callback),
            &mut out as *mut Vec<String> as *mut libc::c_void,
        );
    }
    shared_objects(out)
}

/// `dl_iterate_phdr` entries → shared libraries. The first entry is always the
/// main program (glibc names it `""`, static musl `/proc/self/exe`), and the
/// kernel's vDSO is not a file, so both are dropped.
#[cfg_attr(
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )),
    allow(dead_code)
)]
fn shared_objects(entries: Vec<String>) -> Vec<String> {
    entries
        .into_iter()
        .skip(1)
        .filter(|n| !n.is_empty() && !n.starts_with("linux-vdso") && !n.starts_with("linux-gate"))
        .collect()
}

#[cfg(target_vendor = "apple")]
fn loaded_libraries() -> Vec<String> {
    // <mach-o/dyld.h>, in libSystem. Declared here because libc's bindings
    // are deprecated in favour of a crate we do not otherwise need.
    extern "C" {
        fn _dyld_image_count() -> u32;
        fn _dyld_get_image_name(image_index: u32) -> *const libc::c_char;
    }
    let mut out = Vec::new();
    // SAFETY: dyld's image list; names are static NUL-terminated strings.
    // Image 0 is the main executable, so it is skipped.
    unsafe {
        let n = _dyld_image_count();
        for i in 1..n {
            let p = _dyld_get_image_name(i);
            if !p.is_null() {
                out.push(std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned());
            }
        }
    }
    out
}

#[cfg(windows)]
fn loaded_libraries() -> Vec<String> {
    use std::ffi::c_void;
    type Handle = *mut c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn K32EnumProcessModules(
            process: Handle,
            modules: *mut Handle,
            cb: u32,
            needed: *mut u32,
        ) -> i32;
        fn GetModuleFileNameW(module: Handle, name: *mut u16, size: u32) -> u32;
    }
    let mut out = Vec::new();
    // SAFETY: Win32 module enumeration with caller-owned buffers sized by the
    // `needed` count the first call reports.
    unsafe {
        let proc_ = GetCurrentProcess();
        let mut needed = 0u32;
        if K32EnumProcessModules(proc_, std::ptr::null_mut(), 0, &mut needed) == 0 {
            return out;
        }
        let count = needed as usize / std::mem::size_of::<Handle>();
        let mut mods: Vec<Handle> = vec![std::ptr::null_mut(); count];
        if K32EnumProcessModules(proc_, mods.as_mut_ptr(), needed, &mut needed) == 0 {
            return out;
        }
        // Module 0 is the main executable.
        for &m in mods.iter().skip(1) {
            let mut buf = [0u16; 1024];
            let len = GetModuleFileNameW(m, buf.as_mut_ptr(), buf.len() as u32) as usize;
            if len > 0 {
                out.push(String::from_utf16_lossy(&buf[..len]));
            }
        }
    }
    out
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_vendor = "apple",
    windows
)))]
fn loaded_libraries() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_process_facts() {
        let e = RuntimeEnv::collect();
        assert_eq!(e.os, std::env::consts::OS);
        assert_eq!(e.pid, std::process::id());
        assert!(e.started_at.ends_with('Z'));
        assert!(!e.exe_path.is_empty());
    }

    #[test]
    fn main_program_and_vdso_are_not_libraries() {
        // What a static musl binary reports: only itself.
        assert!(shared_objects(vec!["/proc/self/exe".into()]).is_empty());
        // A dynamic glibc binary: main program (""), vDSO, then real libraries.
        assert_eq!(
            shared_objects(vec![
                "".into(),
                "linux-vdso.so.1".into(),
                "/lib/x86_64-linux-gnu/libc.so.6".into(),
                "/lib64/ld-linux-x86-64.so.2".into(),
            ]),
            [
                "/lib/x86_64-linux-gnu/libc.so.6",
                "/lib64/ld-linux-x86-64.so.2"
            ]
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn loaded_libraries_exclude_the_executable() {
        let e = RuntimeEnv::collect();
        let exe = std::fs::canonicalize(&e.exe_path).unwrap();
        for l in &e.loaded_libraries {
            assert_ne!(
                std::fs::canonicalize(l).ok().as_ref(),
                Some(&exe),
                "the executable is listed as a library: {l}"
            );
        }
        assert!(!e.loaded_libraries.iter().any(|l| l == "/proc/self/exe"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn loaded_libraries_include_libc() {
        let libs = RuntimeEnv::collect().loaded_libraries;
        assert!(!libs.is_empty(), "no loaded libraries reported");
        let want = if cfg!(target_os = "macos") {
            "libSystem"
        } else {
            "libc."
        };
        assert!(
            libs.iter().any(|l| l.contains(want)),
            "{want} missing from {libs:?}"
        );
    }

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[test]
    fn glibc_version_reported() {
        assert!(RuntimeEnv::collect().libc_version.is_some());
    }

    #[cfg(feature = "exe-digest")]
    #[test]
    fn exe_digest_is_sha256_hex() {
        let d = RuntimeEnv::collect().exe_digest.expect("digest");
        assert_eq!(d.len(), 64);
    }
}
