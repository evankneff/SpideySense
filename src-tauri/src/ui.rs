//! Native message boxes.
//!
//! Release builds have no console (`windows_subsystem = "windows"`) and there is no
//! main window, so a startup failure would otherwise be completely silent. Everything
//! fatal goes through here as well as into the log.

/// UTF-16, NUL-terminated - what the Win32 `*W` entry points expect.
#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn message_box(title: &str, body: &str, icon: u32) {
    use ::windows::core::PCWSTR;
    use ::windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MESSAGEBOX_STYLE,
    };

    let title = wide(title);
    let body = wide(body);

    // SAFETY: both pointers reference NUL-terminated buffers that outlive the call.
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_TOPMOST | MB_SETFOREGROUND | MESSAGEBOX_STYLE(icon),
        );
    }
}

#[cfg(not(windows))]
fn message_box(title: &str, body: &str, _icon: u32) {
    eprintln!("{title}\n\n{body}");
}

pub fn error(title: &str, body: &str) {
    #[cfg(windows)]
    let icon = ::windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR.0;
    #[cfg(not(windows))]
    let icon = 0;
    message_box(title, body, icon);
}

pub fn info(title: &str, body: &str) {
    #[cfg(windows)]
    let icon = ::windows::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION.0;
    #[cfg(not(windows))]
    let icon = 0;
    message_box(title, body, icon);
}
