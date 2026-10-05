use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::mpsc,
    time::{Duration, Instant},
};

pub fn replace_file(from: &Path, to: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let from = wide(&from.to_string_lossy());
        let to = wide(&to.to_string_lossy());
        if unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(not(windows))]
    fs::rename(from, to)?;
    Ok(())
}
pub fn command(binary: &Path, args: &[String]) -> Result<Command> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let ext = binary
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        let mut cmd = if matches!(ext.as_str(), "cmd" | "bat") {
            let path = binary.to_string_lossy();
            if std::iter::once(path.as_ref())
                .chain(args.iter().map(String::as_str))
                .any(|s| s.contains(['"', '%', '\r', '\n']))
            {
                bail!("批处理路径包含不支持的字符，请指定原生 codex.exe");
            }
            let line = std::iter::once(path.as_ref())
                .chain(args.iter().map(String::as_str))
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(" ");
            let mut cmd =
                Command::new(std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into()));
            cmd.args(["/d", "/v:off", "/s", "/c"])
                .raw_arg(format!("\"{line}\""));
            cmd
        } else if ext == "ps1" {
            let mut cmd = Command::new("powershell.exe");
            cmd.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
                .arg(binary)
                .args(args);
            cmd
        } else {
            let mut cmd = Command::new(binary);
            cmd.args(args);
            cmd
        };
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        Ok(cmd)
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new(binary);
        cmd.args(args);
        Ok(cmd)
    }
}

#[cfg(windows)]
pub struct Job(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Job {
    pub fn assign(child: &Child) -> Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let job = Self(handle);
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as _,
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
            || unsafe { AssignProcessToJobObject(handle, child.as_raw_handle() as _) } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(job)
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(not(windows))]
pub struct Job;
#[cfg(not(windows))]
impl Job {
    pub fn assign(_child: &Child) -> Result<Self> {
        Ok(Self)
    }
}

#[derive(Clone, Debug)]
pub struct Binary {
    pub path: PathBuf,
    pub version: String,
    pub no_daemon: bool,
}
fn output(binary: &Path, arg: &str) -> Result<String> {
    let mut child = command(binary, &[arg.into()])?
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let job = match Job::assign(&child) {
        Ok(j) => j,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let stdout = child.stdout.take().context("missing stdout")?;
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut text = String::new();
        stdout
            .take(256 * 1024)
            .read_to_string(&mut text)
            .map(|_| text)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            drop(job);
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            bail!("Codex 版本检查超时");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    drop(job);
    reader
        .join()
        .map_err(|_| anyhow::anyhow!("版本读取线程退出"))?
        .map_err(Into::into)
}
fn version_key(s: &str) -> (u32, u32, u32, bool) {
    let parts = s
        .split(['.', '-'])
        .take(3)
        .map(|s| s.parse::<u32>().unwrap_or(0))
        .collect::<Vec<_>>();
    (
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
        !s.contains('-'),
    )
}
pub fn discover(configured: &str) -> Result<Option<Binary>> {
    let mut paths = Vec::new();
    if !configured.is_empty() {
        paths.push(PathBuf::from(configured));
    } else {
        for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
            for name in if cfg!(windows) {
                vec!["codex.exe", "codex.cmd", "codex.ps1"]
            } else {
                vec!["codex"]
            } {
                let path = dir.join(name);
                if path.is_file() {
                    paths.push(path);
                }
            }
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let root = PathBuf::from(local).join("OpenAI/Codex/bin");
            if let Ok(dirs) = fs::read_dir(root) {
                for d in dirs.flatten() {
                    let p = d.path().join("codex.exe");
                    if p.is_file() {
                        paths.push(p);
                    }
                }
            }
        }
        if let Some(programs) = std::env::var_os("ProgramFiles")
            && let Ok(dirs) = fs::read_dir(PathBuf::from(programs).join("WindowsApps"))
        {
            for d in dirs
                .flatten()
                .filter(|d| d.file_name().to_string_lossy().starts_with("OpenAI.Codex_"))
            {
                for rel in [
                    "app/resources/codex.exe",
                    "app/resources/codex-cli/bin/codex.exe",
                ] {
                    let p = d.path().join(rel);
                    if p.is_file() {
                        paths.push(p);
                    }
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    let re = regex::Regex::new(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?")?;
    let mut found = Vec::new();
    for path in paths {
        if let Ok(text) = output(&path, "--version")
            && let Some(m) = re.find(&text)
        {
            found.push(Binary {
                path,
                version: m.as_str().into(),
                no_daemon: false,
            });
        }
    }
    found.sort_by(|a, b| {
        version_key(&b.version)
            .cmp(&version_key(&a.version))
            .then_with(|| {
                // At equal versions prefer the native EXE over a shell wrapper.
                (a.path.extension().is_none_or(|s| s != "exe"))
                    .cmp(&b.path.extension().is_none_or(|s| s != "exe"))
            })
    });
    let Some(mut binary) = found.into_iter().next() else {
        return Ok(None);
    };
    binary.no_daemon = output(&binary.path, "--help")?.contains("--no-daemon");
    Ok(Some(binary))
}
pub fn show_error(message: &str) {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            std::ptr::null_mut(),
            wide(message).as_ptr(),
            wide("is-gpt-nerfed").as_ptr(),
            0x10,
        );
    }
    #[cfg(not(windows))]
    eprintln!("{message}");
}
#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
pub fn open_folder(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        command(
            Path::new("explorer.exe"),
            &[path.to_string_lossy().into_owned()],
        )?
        .spawn()?;
    }
    #[cfg(not(windows))]
    {
        Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum TrayEvent {
    Show,
    ToggleAutomatic,
    Quit,
}
pub fn icon_rgba() -> Vec<u8> {
    let mut pixels = Vec::with_capacity(32 * 32 * 4);
    for y in 0_i32..32 {
        for x in 0_i32..32 {
            let inside = (x - x.clamp(5, 26)).pow(2) + (y - y.clamp(5, 26)).pow(2) <= 25;
            let bar = ((8..12).contains(&x) && (17..25).contains(&y))
                || ((14..18).contains(&x) && (12..25).contains(&y))
                || ((20..24).contains(&x) && (7..25).contains(&y));
            pixels.extend(if !inside {
                [0, 0, 0, 0]
            } else if bar {
                [255, 255, 255, 255]
            } else {
                [21, 134, 115, 255]
            });
        }
    }
    pixels
}
#[cfg(windows)]
mod tray {
    use super::*;
    use windows_sys::Win32::{
        Foundation::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{Shell::*, WindowsAndMessaging::*},
    };
    const CALLBACK: u32 = WM_APP + 1;
    const NOTIFY: u32 = WM_APP + 2;
    struct State {
        tx: mpsc::Sender<TrayEvent>,
        icon: NOTIFYICONDATAW,
        taskbar_created: u32,
    }
    fn fill<const N: usize>(dst: &mut [u16; N], s: &str) {
        for (d, c) in dst.iter_mut().take(N - 1).zip(s.encode_utf16()) {
            *d = c;
        }
    }
    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe {
            if msg == WM_CREATE {
                let cs = &*(l as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
                return 0;
            }
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
            if !ptr.is_null() {
                // Popup menus dispatch messages reentrantly; keep only a shared reference here.
                let state = &*ptr;
                if state.taskbar_created != 0 && msg == state.taskbar_created {
                    // Explorer restart and primary-display DPI changes remove notification icons.
                    Shell_NotifyIconW(NIM_ADD, &state.icon);
                    return 0;
                }
                match msg {
                    CALLBACK => {
                        if matches!(
                            l as u32,
                            WM_LBUTTONUP | WM_LBUTTONDBLCLK | NIN_BALLOONUSERCLICK
                        ) {
                            let _ = state.tx.send(TrayEvent::Show);
                        }
                        if l as u32 == WM_RBUTTONUP {
                            let menu = CreatePopupMenu();
                            AppendMenuW(menu, MF_STRING, 1, wide("打开窗口").as_ptr());
                            AppendMenuW(menu, MF_STRING, 2, wide("切换自动检测").as_ptr());
                            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                            AppendMenuW(menu, MF_STRING, 3, wide("退出程序").as_ptr());
                            let mut pos: POINT = std::mem::zeroed();
                            GetCursorPos(&mut pos);
                            SetForegroundWindow(hwnd);
                            TrackPopupMenu(menu, 0, pos.x, pos.y, 0, hwnd, std::ptr::null());
                            DestroyMenu(menu);
                            PostMessageW(hwnd, WM_NULL, 0, 0);
                        }
                        return 0;
                    }
                    WM_COMMAND => {
                        let event = match w & 0xffff {
                            1 => Some(TrayEvent::Show),
                            2 => Some(TrayEvent::ToggleAutomatic),
                            3 => Some(TrayEvent::Quit),
                            _ => None,
                        };
                        if let Some(event) = event {
                            let _ = state.tx.send(event);
                        }
                        return 0;
                    }
                    NOTIFY => {
                        let message = Box::from_raw(l as *mut String);
                        let mut icon = state.icon;
                        icon.uFlags = NIF_INFO;
                        icon.szInfo = [0; 256];
                        icon.szInfoTitle = [0; 64];
                        fill(&mut icon.szInfo, &message);
                        fill(&mut icon.szInfoTitle, "is-gpt-nerfed 检测结果");
                        icon.dwInfoFlags = NIIF_INFO;
                        Shell_NotifyIconW(NIM_MODIFY, &icon);
                        return 0;
                    }
                    WM_CLOSE => {
                        Shell_NotifyIconW(NIM_DELETE, &state.icon);
                        DestroyWindow(hwnd);
                        return 0;
                    }
                    WM_DESTROY => {
                        PostQuitMessage(0);
                        return 0;
                    }
                    _ => {}
                }
            }
            DefWindowProcW(hwnd, msg, w, l)
        }
    }
    pub struct Tray {
        hwnd: isize,
        pub events: mpsc::Receiver<TrayEvent>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl Tray {
        pub fn new() -> Result<Self> {
            let (tx, events) = mpsc::channel();
            let (ready_tx, ready_rx) = mpsc::channel();
            let thread = std::thread::spawn(move || unsafe {
                let instance = GetModuleHandleW(std::ptr::null());
                let class = wide("IsGPTNerfedTray");
                let mut wc: WNDCLASSW = std::mem::zeroed();
                wc.lpfnWndProc = Some(proc);
                wc.hInstance = instance;
                wc.lpszClassName = class.as_ptr();
                RegisterClassW(&wc);
                let mut state = Box::new(State {
                    tx,
                    icon: std::mem::zeroed(),
                    taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
                });
                let hwnd = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    wide("IsGPTNerfedTray").as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    instance,
                    &mut *state as *mut State as _,
                );
                if hwnd.is_null() {
                    let _ = ready_tx.send(Err(std::io::Error::last_os_error().to_string()));
                    return;
                }
                state.icon.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                state.icon.hWnd = hwnd;
                state.icon.uID = 1;
                state.icon.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
                state.icon.uCallbackMessage = CALLBACK;
                let rgba = icon_rgba();
                let mut mask = [255_u8; 128];
                let mut bgra = Vec::with_capacity(rgba.len());
                for (i, pixel) in rgba.chunks_exact(4).enumerate() {
                    if pixel[3] != 0 {
                        mask[i / 8] &= !(1 << (7 - i % 8));
                    }
                    bgra.extend([pixel[2], pixel[1], pixel[0], pixel[3]]);
                }
                let owned_icon = CreateIcon(instance, 32, 32, 1, 32, mask.as_ptr(), bgra.as_ptr());
                state.icon.hIcon = if owned_icon.is_null() {
                    LoadIconW(std::ptr::null_mut(), IDI_INFORMATION)
                } else {
                    owned_icon
                };
                fill(&mut state.icon.szTip, "is-gpt-nerfed · 点击打开，右键退出");
                if Shell_NotifyIconW(NIM_ADD, &state.icon) == 0 {
                    DestroyWindow(hwnd);
                    if !owned_icon.is_null() {
                        DestroyIcon(owned_icon);
                    }
                    let _ = ready_tx.send(Err("Windows 无法创建托盘图标".into()));
                    return;
                }
                let _ = ready_tx.send(Ok(hwnd as isize));
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                if !owned_icon.is_null() {
                    DestroyIcon(owned_icon);
                }
                UnregisterClassW(class.as_ptr(), instance);
            });
            let hwnd = ready_rx
                .recv_timeout(Duration::from_secs(5))?
                .map_err(anyhow::Error::msg)?;
            Ok(Self {
                hwnd,
                events,
                thread: Some(thread),
            })
        }
        pub fn notify(&self, message: String) {
            unsafe {
                let ptr = Box::into_raw(Box::new(message));
                if PostMessageW(self.hwnd as HWND, NOTIFY, 0, ptr as isize) == 0 {
                    drop(Box::from_raw(ptr));
                }
            }
        }
    }
    impl Drop for Tray {
        fn drop(&mut self) {
            unsafe {
                PostMessageW(self.hwnd as HWND, WM_CANCELMODE, 0, 0);
                PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0);
            }
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn taskbar_recreation_and_notification_click_keep_the_tray_usable() {
            let tray = Tray::new().unwrap();
            unsafe {
                let mut icon: NOTIFYICONDATAW = std::mem::zeroed();
                icon.cbSize = std::mem::size_of_val(&icon) as u32;
                icon.hWnd = tray.hwnd as HWND;
                icon.uID = 1;
                assert_ne!(Shell_NotifyIconW(NIM_DELETE, &icon), 0);
                let message = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
                assert_ne!(PostMessageW(icon.hWnd, message, 0, 0), 0);
                let mut identity: NOTIFYICONIDENTIFIER = std::mem::zeroed();
                identity.cbSize = std::mem::size_of_val(&identity) as u32;
                identity.hWnd = icon.hWnd;
                identity.uID = 1;
                let deadline = Instant::now() + Duration::from_secs(3);
                loop {
                    let mut rect: RECT = std::mem::zeroed();
                    if Shell_NotifyIconGetRect(&identity, &mut rect) >= 0 {
                        break;
                    }
                    assert!(Instant::now() < deadline, "tray icon was not restored");
                    std::thread::sleep(Duration::from_millis(25));
                }
                assert_ne!(
                    PostMessageW(icon.hWnd, CALLBACK, 0, NIN_BALLOONUSERCLICK as isize),
                    0
                );
            }
            assert!(matches!(
                tray.events.recv_timeout(Duration::from_secs(2)).unwrap(),
                TrayEvent::Show
            ));
        }
    }
}
#[cfg(windows)]
pub use tray::Tray;
#[cfg(not(windows))]
pub struct Tray {
    pub events: mpsc::Receiver<TrayEvent>,
}
#[cfg(not(windows))]
impl Tray {
    pub fn new() -> Result<Self> {
        let (_, events) = mpsc::channel();
        Ok(Self { events })
    }
    pub fn notify(&self, _message: String) {}
}
