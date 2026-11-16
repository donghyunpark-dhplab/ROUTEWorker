//! 관리자 권한 확인 및 UAC 재실행.

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

/// 현재 프로세스가 관리자 권한(상승된 토큰)으로 실행 중인지.
pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut _),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// UAC 프롬프트를 띄워 자기 자신을 관리자 권한으로 다시 실행한다.
/// 성공하면 호출자가 현재 프로세스를 종료해야 한다.
pub fn relaunch_as_admin() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("실행 파일 경로 확인 실패: {e}"))?;
    let exe = HSTRING::from(exe.as_os_str());
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("runas"),
            &exe,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute는 32보다 큰 값을 돌려주면 성공이다.
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err("관리자 권한으로 다시 시작하지 못했습니다 (UAC 취소 또는 거부).".to_owned())
    }
}
