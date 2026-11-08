use std::io;

pub(crate) async fn reduced_motion() -> io::Result<bool> {
    #[cfg(target_os = "macos")]
    {
        use objc::{class, msg_send, sel, sel_impl};
        let reduced: objc::runtime::BOOL = unsafe {
            let workspace: *mut objc::runtime::Object =
                msg_send![class!(NSWorkspace), sharedWorkspace];
            msg_send![workspace, accessibilityDisplayShouldReduceMotion]
        };
        Ok(reduced != objc::runtime::NO)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
        };
        let mut enabled = 1i32;
        if unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut enabled as *mut i32).cast(),
                0,
            )
        } == 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(enabled == 0)
        }
    }
    #[cfg(target_os = "linux")]
    {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_ascii_lowercase();
        super::fs_task::run(
            "Read desktop animation preference",
            "desktop-preferences".into(),
            move || {
                let output = if desktop.contains("kde") {
                    let args = [
                        "--file",
                        "kdeglobals",
                        "--group",
                        "KDE",
                        "--key",
                        "AnimationDurationFactor",
                        "--default",
                        "1",
                    ];
                    match std::process::Command::new("kreadconfig6")
                        .args(args)
                        .output()
                    {
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {
                            std::process::Command::new("kreadconfig5")
                                .args(args)
                                .output()?
                        }
                        result => result?,
                    }
                } else if desktop.contains("gnome")
                    || desktop.contains("cinnamon")
                    || desktop.contains("unity")
                {
                    let schema = if desktop.contains("cinnamon") {
                        "org.cinnamon.desktop.interface"
                    } else {
                        "org.gnome.desktop.interface"
                    };
                    std::process::Command::new("gsettings")
                        .args(["get", schema, "enable-animations"])
                        .output()?
                } else {
                    return Ok(false);
                };
                if !output.status.success() {
                    return Err(io::Error::other(crate::i18n::runtime::rt!(
                        InvalidSetting,
                        output.status
                    )));
                }
                let text = String::from_utf8(output.stdout).map_err(io::Error::other)?;
                parse_preference(&text, desktop.contains("kde"))
            },
        )
        .await
    }
}

#[cfg(any(target_os = "linux", test))]
fn parse_preference(value: &str, duration_factor: bool) -> io::Result<bool> {
    let value = value.trim();
    if duration_factor {
        return value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| value == 0.0)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    crate::i18n::runtime::RuntimeText::InvalidSetting.to_string(),
                )
            });
    }
    match value {
        "true" => Ok(false),
        "false" => Ok(true),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            crate::i18n::runtime::RuntimeText::InvalidSetting.to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_preferences_preserve_disabled_and_reject_invalid_values() {
        assert!(parse_preference("false\n", false).unwrap());
        assert!(!parse_preference("true", false).unwrap());
        assert!(parse_preference("0", true).unwrap());
        assert!(!parse_preference("0.5", true).unwrap());
        assert!(parse_preference("NaN", true).is_err());
        assert!(parse_preference("unknown", false).is_err());
    }
}
