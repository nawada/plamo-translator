//! 前面アプリで選択中の文字列を取得する。
//!
//! Windows では Ctrl+C を擬似入力してクリップボード経由で取得し、
//! 取得後は元のクリップボードの内容を復元する。

#[cfg(windows)]
mod imp {
    use std::thread::sleep;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

    enum Saved {
        Text(String),
        Image(arboard::ImageData<'static>),
        Empty,
    }

    fn key_down(vk: u16) -> bool {
        unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
    }

    /// ショートカットのキーが物理的に離されるのを待つ (最大 `timeout`)
    fn wait_keys_released(timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            // 修飾キーとショートカットに使われうる通常キーだけを見る
            // (IME 関連キーは押下状態が残り続けることがあるため除外)
            let any_down = [0x10u16..=0x12, 0x20..=0x5C, 0x70..=0x87, 0xA0..=0xA5, 0xBA..=0xC0, 0xDB..=0xDF]
                .into_iter()
                .flatten()
                .any(key_down);
            if !any_down {
                return true;
            }
            if start.elapsed() > timeout {
                return false;
            }
            sleep(Duration::from_millis(10));
        }
    }

    fn kb(vk: u16, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn send(inputs: &[INPUT]) {
        unsafe {
            SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        }
    }

    fn send_ctrl_c() {
        let mut inputs = Vec::new();
        // まだ押されている修飾キーを離す (Ctrl+Shift+C などにならないように)
        for vk in [VK_LSHIFT, VK_RSHIFT, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN, VK_LCONTROL, VK_RCONTROL] {
            if key_down(vk) {
                inputs.push(kb(vk, true));
            }
        }
        // Alt を離したときにメニューバーが反応しないよう、未割り当てキーを挟む
        inputs.push(kb(0xE8, false));
        inputs.push(kb(0xE8, true));
        inputs.push(kb(VK_CONTROL, false));
        inputs.push(kb(b'C' as u16, false));
        inputs.push(kb(b'C' as u16, true));
        inputs.push(kb(VK_CONTROL, true));
        send(&inputs);
    }

    pub fn capture() -> Option<String> {
        let mut cb = arboard::Clipboard::new().ok()?;
        let saved = match cb.get_text() {
            Ok(t) => Saved::Text(t),
            Err(_) => match cb.get_image() {
                Ok(img) => Saved::Image(img.to_owned_img()),
                Err(_) => Saved::Empty,
            },
        };

        wait_keys_released(Duration::from_millis(800));
        let before = unsafe { GetClipboardSequenceNumber() };
        send_ctrl_c();

        let start = Instant::now();
        let mut changed = false;
        while start.elapsed() < Duration::from_millis(700) {
            sleep(Duration::from_millis(15));
            if unsafe { GetClipboardSequenceNumber() } != before {
                changed = true;
                break;
            }
        }
        if !changed {
            return None;
        }

        // コピー元アプリが書き込みを終えるのを少し待ってから読む
        let mut text = None;
        for _ in 0..10 {
            sleep(Duration::from_millis(20));
            match cb.get_text() {
                Ok(t) => {
                    text = Some(t);
                    break;
                }
                Err(arboard::Error::ContentNotAvailable) => break,
                Err(_) => continue,
            }
        }

        // クリップボードを元に戻す
        let _ = match saved {
            Saved::Text(t) => cb.set_text(t),
            Saved::Image(img) => cb.set_image(img),
            Saved::Empty => cb.clear(),
        };

        text.filter(|t| !t.trim().is_empty())
    }
}

#[cfg(windows)]
pub fn capture_selection() -> Option<String> {
    imp::capture()
}

#[cfg(not(windows))]
pub fn capture_selection() -> Option<String> {
    None
}
