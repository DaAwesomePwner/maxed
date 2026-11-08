//! The only unsafe boundary. Npcap resources remain on their creating thread.
use crate::{
    live::{AdapterInfo, Backend, Device},
    CaptureError,
};
use maxed_types::{RawFrame, Timestamp};
#[cfg(any(windows, test))]
use std::path::Path;
use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    rc::Rc,
};

#[repr(C)]
pub struct Timeval {
    pub seconds: i32,
    pub micros: i32,
}
#[repr(C)]
pub struct PcapHeader {
    pub ts: Timeval,
    pub caplen: u32,
    pub len: u32,
}
#[repr(C)]
pub struct BpfProgram {
    pub len: u32,
    pub insns: *mut c_void,
}
#[repr(C)]
pub struct PcapIf {
    pub next: *mut PcapIf,
    pub name: *mut c_char,
    pub description: *mut c_char,
    pub addresses: *mut c_void,
    pub flags: u32,
}
// Windows pcap_stat includes the WinPcap remote-capture extension fields.
#[repr(C)]
#[derive(Default)]
struct PcapStat {
    ps_recv: u32,
    ps_drop: u32,
    ps_ifdrop: u32,
    ps_capt: u32,
    ps_sent: u32,
    ps_netdrop: u32,
}
type Stats = unsafe extern "C" fn(*mut c_void, *mut PcapStat) -> c_int;
type FindAll = unsafe extern "C" fn(*mut *mut PcapIf, *mut c_char) -> c_int;
type FreeAll = unsafe extern "C" fn(*mut PcapIf);
type OpenLive =
    unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, *mut c_char) -> *mut c_void;
type Compile =
    unsafe extern "C" fn(*mut c_void, *mut BpfProgram, *const c_char, c_int, u32) -> c_int;
type SetFilter = unsafe extern "C" fn(*mut c_void, *mut BpfProgram) -> c_int;
type FreeCode = unsafe extern "C" fn(*mut BpfProgram);
type NextEx = unsafe extern "C" fn(*mut c_void, *mut *const PcapHeader, *mut *const u8) -> c_int;
type Close = unsafe extern "C" fn(*mut c_void);
type GetErr = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type Datalink = unsafe extern "C" fn(*mut c_void) -> c_int;
type SetNonblock = unsafe extern "C" fn(*mut c_void, c_int, *mut c_char) -> c_int;

#[cfg(any(windows, test))]
const SYMBOLS: [&[u8]; 12] = [
    b"pcap_findalldevs\0",
    b"pcap_freealldevs\0",
    b"pcap_open_live\0",
    b"pcap_compile\0",
    b"pcap_setfilter\0",
    b"pcap_freecode\0",
    b"pcap_next_ex\0",
    b"pcap_stats\0",
    b"pcap_close\0",
    b"pcap_geterr\0",
    b"pcap_datalink\0",
    b"pcap_setnonblock\0",
];

#[cfg(any(windows, test))]
fn validate_symbols(mut has: impl FnMut(&[u8]) -> bool) -> Result<(), CaptureError> {
    for symbol in SYMBOLS {
        if !has(symbol) {
            return Err(CaptureError::Incompatible);
        }
    }
    Ok(())
}
#[cfg(any(windows, test))]
fn load_candidates<T>(
    root: &Path,
    mut load: impl FnMut(&Path) -> Result<T, CaptureError>,
) -> Result<T, CaptureError> {
    let mut failure = CaptureError::Missing;
    for path in crate::npcap_candidates(root) {
        match load(&path) {
            Ok(api) => return Ok(api),
            Err(error) => {
                if error != CaptureError::Missing {
                    failure = error;
                }
            }
        }
    }
    Err(failure)
}

pub(crate) struct Api {
    // Kept alive until the last native resource has been dropped.
    _library: libloading::Library,
    find_all: FindAll,
    free_all: FreeAll,
    open_live: OpenLive,
    compile: Compile,
    set_filter: SetFilter,
    free_code: FreeCode,
    next_ex: NextEx,
    stats: Stats,
    close: Close,
    get_err: GetErr,
    datalink: Datalink,
    set_nonblock: SetNonblock,
}
impl Api {
    #[cfg(windows)]
    pub(crate) fn load() -> Result<Self, CaptureError> {
        let root = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:/Windows".into());
        load_candidates(&root, |path| {
            // LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS.
            let flags = if path.is_absolute() {
                0x100 | 0x1000
            } else {
                0x1000
            };
            // SAFETY: Loading installed Npcap is the explicit live-capture boundary.
            let library: libloading::Library =
                unsafe { libloading::os::windows::Library::load_with_flags(path, flags) }
                    .map_err(|error| {
                        use std::error::Error;
                        let code = error
                            .source()
                            .and_then(|source| source.downcast_ref::<std::io::Error>())
                            .and_then(std::io::Error::raw_os_error);
                        match code {
                            Some(2 | 3 | 126) if !path.exists() => CaptureError::Missing,
                            Some(5) => CaptureError::PermissionDenied,
                            _ => CaptureError::Incompatible,
                        }
                    })?
                    .into();
            // SAFETY: Only presence is checked here; symbols are bound to Npcap ABI below.
            validate_symbols(|name| unsafe { library.get::<*const c_void>(name).is_ok() })?;
            macro_rules! symbol { ($name:literal, $ty:ty) => {{
                // SAFETY: Signatures are the documented Win32 pcap ABI. The library is retained.
                unsafe { *library.get::<$ty>(concat!($name, "\0").as_bytes()).map_err(|_| CaptureError::Incompatible)? }
            }}; }
            Ok(Self {
                find_all: symbol!("pcap_findalldevs", FindAll),
                free_all: symbol!("pcap_freealldevs", FreeAll),
                open_live: symbol!("pcap_open_live", OpenLive),
                compile: symbol!("pcap_compile", Compile),
                set_filter: symbol!("pcap_setfilter", SetFilter),
                free_code: symbol!("pcap_freecode", FreeCode),
                next_ex: symbol!("pcap_next_ex", NextEx),
                stats: symbol!("pcap_stats", Stats),
                close: symbol!("pcap_close", Close),
                get_err: symbol!("pcap_geterr", GetErr),
                datalink: symbol!("pcap_datalink", Datalink),
                set_nonblock: symbol!("pcap_setnonblock", SetNonblock),
                _library: library,
            })
        })
    }
    #[cfg(not(windows))]
    pub(crate) fn load() -> Result<Self, CaptureError> {
        Err(CaptureError::UnsupportedPlatform)
    }
}

fn native_error(bytes: &[c_char]) -> CaptureError {
    // Classification only; native strings never leave the FFI boundary.
    let text: String = bytes
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| (*b as u8 as char).to_ascii_lowercase())
        .collect();
    if text.contains("denied")
        || text.contains("permission")
        || text.contains("administrator")
        || text.contains("access is")
    {
        CaptureError::PermissionDenied
    } else {
        CaptureError::OpenFailed
    }
}

struct AdapterList<'a> {
    api: &'a Api,
    head: *mut PcapIf,
}
impl Drop for AdapterList<'_> {
    fn drop(&mut self) {
        if !self.head.is_null() {
            // SAFETY: Owned list allocated by this API; freed once.
            unsafe { (self.api.free_all)(self.head) };
        }
    }
}
struct Program<'a> {
    api: &'a Api,
    program: BpfProgram,
}
impl Drop for Program<'_> {
    fn drop(&mut self) {
        if !self.program.insns.is_null() {
            // SAFETY: Compiled program belongs to this API, including partial failure allocations.
            unsafe { (self.api.free_code)(&mut self.program) };
        }
    }
}
pub(crate) struct NativeBackend {
    api: Rc<Api>,
}
impl NativeBackend {
    pub(crate) fn load() -> Result<Self, CaptureError> {
        Ok(Self {
            api: Rc::new(Api::load()?),
        })
    }
}
impl Backend for NativeBackend {
    fn discover(&mut self) -> Result<Vec<AdapterInfo>, CaptureError> {
        let mut error = [0; 256];
        let mut list = AdapterList {
            api: &self.api,
            head: std::ptr::null_mut(),
        };
        // SAFETY: Writable out pointer and pcap-sized error buffer.
        if unsafe { (self.api.find_all)(&mut list.head, error.as_mut_ptr()) } != 0 {
            return Err(native_error(&error));
        }
        let mut adapters = Vec::new();
        let mut current = list.head;
        let mut seen = std::collections::HashSet::new();
        while !current.is_null() {
            if seen.len() >= 4096 || !seen.insert(current) {
                return Err(CaptureError::Incompatible);
            }
            // SAFETY: Current points into the live pcap-owned list; no pointers escape.
            let entry = unsafe { &*current };
            if entry.flags & 2 != 0 && entry.flags & 1 == 0 && !entry.name.is_null() {
                // SAFETY: pcap documents these list fields as NUL-terminated strings.
                let id = unsafe { CStr::from_ptr(entry.name) }
                    .to_string_lossy()
                    .into_owned();
                let description = if entry.description.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(entry.description) }
                        .to_string_lossy()
                        .into_owned()
                };
                adapters.push(AdapterInfo { id, description });
            }
            current = entry.next;
        }
        if adapters.is_empty() {
            Err(CaptureError::NoAdapters)
        } else {
            Ok(adapters)
        }
    }
    fn open(&mut self, adapter: &AdapterInfo) -> Result<Box<dyn Device>, CaptureError> {
        let name = CString::new(adapter.id.as_bytes()).map_err(|_| CaptureError::InvalidConfig)?;
        let mut error = [0; 256];
        // SAFETY: Input name and error buffer live through the call. Non-promiscuous capture.
        let handle =
            unsafe { (self.api.open_live)(name.as_ptr(), 65_535, 0, 10, error.as_mut_ptr()) };
        if handle.is_null() {
            return Err(native_error(&error));
        }
        let device = NativeDevice {
            api: self.api.clone(),
            handle,
        };
        // SAFETY: Handle was checked and belongs to this thread.
        if unsafe { (self.api.datalink)(handle) } != 1 {
            return Err(CaptureError::Incompatible);
        }
        let mut program = Program {
            api: &self.api,
            program: BpfProgram {
                len: 0,
                insns: std::ptr::null_mut(),
            },
        };
        // SAFETY: Exact constant filter, initialized program and valid handle.
        if unsafe {
            (self.api.compile)(
                handle,
                &mut program.program,
                c"tcp port 2050".as_ptr(),
                1,
                u32::MAX,
            )
        } != 0
        {
            return Err(CaptureError::OpenFailed);
        }
        // SAFETY: Valid compiled program lives through filter installation.
        if unsafe { (self.api.set_filter)(handle, &mut program.program) } != 0 {
            return Err(CaptureError::OpenFailed);
        }
        // SAFETY: Valid handle and writable error buffer; nonblocking prevents idle-adapter starvation.
        if unsafe { (self.api.set_nonblock)(handle, 1, error.as_mut_ptr()) } != 0 {
            return Err(native_error(&error));
        }
        Ok(Box::new(device))
    }
}
struct NativeDevice {
    api: Rc<Api>,
    handle: *mut c_void,
}
impl Drop for NativeDevice {
    fn drop(&mut self) {
        // SAFETY: Non-null handle is owned here and closed once before releasing API.
        unsafe { (self.api.close)(self.handle) };
    }
}
impl Device for NativeDevice {
    fn dropped_packets(&mut self) -> Result<Option<u32>, CaptureError> {
        let mut stats = PcapStat::default();
        // SAFETY: Live handle belongs to this thread/API; output has the Windows pcap_stat layout.
        if unsafe { (self.api.stats)(self.handle, &mut stats) } != 0 {
            return Err(CaptureError::OpenFailed);
        }
        // ps_ifdrop is not consistently available and may cover unfiltered interface traffic.
        // Count the per-handle kernel-buffer loss (ps_drop), not interface-wide estimates.
        Ok(Some(stats.ps_drop))
    }

    fn poll(&mut self) -> Result<Option<RawFrame>, CaptureError> {
        let mut header = std::ptr::null();
        let mut bytes = std::ptr::null();
        // SAFETY: Out pointers and this thread's open handle are valid.
        match unsafe { (self.api.next_ex)(self.handle, &mut header, &mut bytes) } {
            0 => Ok(None),
            1 => {
                if header.is_null() {
                    return Err(CaptureError::OpenFailed);
                }
                // SAFETY: pcap owns a header valid until the next poll.
                let header = unsafe { &*header };
                if header.caplen > 65_535
                    || header.caplen > header.len
                    || header.ts.seconds < 0
                    || !(0..1_000_000).contains(&header.ts.micros)
                    || (header.caplen != 0 && bytes.is_null())
                {
                    return Err(CaptureError::OpenFailed);
                }
                let payload = if header.caplen == 0 {
                    Vec::new()
                } else {
                    // SAFETY: caplen validated against snaplen; pcap guarantees that many readable bytes.
                    unsafe { std::slice::from_raw_parts(bytes, header.caplen as usize) }.to_vec()
                };
                Ok(Some(RawFrame {
                    index: 0,
                    at: Timestamp(header.ts.seconds as u64 * 1_000_000 + header.ts.micros as u64),
                    original_len: header.len,
                    bytes: payload,
                }))
            }
            _ => {
                // SAFETY: Error string is pcap-owned and only classified, never exposed.
                let pointer = unsafe { (self.api.get_err)(self.handle) };
                if pointer.is_null() {
                    return Err(CaptureError::OpenFailed);
                }
                let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
                let chars: Vec<c_char> = bytes.iter().take(256).map(|b| *b as c_char).collect();
                Err(native_error(&chars))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_pcap_layout() {
        assert_eq!(std::mem::size_of::<PcapStat>(), 24);
        assert_eq!(std::mem::offset_of!(PcapStat, ps_drop), 4);
        assert_eq!(std::mem::offset_of!(PcapStat, ps_netdrop), 20);
        assert_eq!(std::mem::size_of::<Timeval>(), 8);
        assert_eq!(std::mem::size_of::<PcapHeader>(), 16);
        assert_eq!(std::mem::offset_of!(PcapHeader, caplen), 8);
    }
    #[test]
    fn injected_loader_classifies_failures_and_checks_every_symbol() {
        let root = Path::new("C:/Windows");
        assert_eq!(
            load_candidates::<()>(root, |_| Err(CaptureError::Missing)),
            Err(CaptureError::Missing)
        );
        assert_eq!(
            load_candidates::<()>(root, |_| Err(CaptureError::Incompatible)),
            Err(CaptureError::Incompatible)
        );
        let mut calls = 0;
        assert_eq!(
            load_candidates(root, |_| {
                calls += 1;
                if calls == 1 {
                    Err(CaptureError::Missing)
                } else {
                    Ok(7)
                }
            }),
            Ok(7)
        );
        assert_eq!(calls, 2);
        for absent in SYMBOLS {
            assert_eq!(
                validate_symbols(|name| name != absent),
                Err(CaptureError::Incompatible)
            );
        }
        assert_eq!(validate_symbols(|_| true), Ok(()));
        assert_eq!(
            native_error(
                &b"Access denied"
                    .iter()
                    .map(|b| *b as c_char)
                    .collect::<Vec<_>>()
            ),
            CaptureError::PermissionDenied
        );
    }
}
