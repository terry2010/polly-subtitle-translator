//! Windows 注册表右键菜单模块
//!
//! 为视频/字幕文件注册 zimufan 右键菜单项：
//! - 视频：`AI-SubTrans 快速翻译`（--mode=quick）
//! - 字幕：`AI-SubTrans 编辑字幕`（--mode=edit）
//!
//! 注册位置：`HKEY_CURRENT_USER\Software\Classes\SystemFileAssociations\.<ext>\shell\zimufan`

use crate::error::AppError;

/// 视频文件扩展名列表
#[cfg(target_os = "windows")]
const VIDEO_EXTENSIONS: &[&str] = &[".mkv", ".mp4", ".avi", ".mov", ".wmv", ".flv", ".ts", ".m2ts"];

/// 字幕文件扩展名列表
#[cfg(target_os = "windows")]
const SUBTITLE_EXTENSIONS: &[&str] = &[".srt", ".ass", ".ssa", ".vtt", ".sub"];

/// 右键菜单在注册表中的子键名
#[cfg(target_os = "windows")]
const SHELL_KEY: &str = "shell\\zimufan";

#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

// === SECTION 1 END ===

#[cfg(target_os = "windows")]
fn register_for_extensions(
    extensions: &[&str],
    menu_label: &str,
    exe_path: &str,
    mode: &str,
) -> Result<(), AppError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let command_value = format!("\"{}\" --mode={} \"%1\"", exe_path, mode);

    for ext in extensions {
        // 路径：Software\Classes\SystemFileAssociations\<ext>\shell\zimufan
        let base = format!(
            "Software\\Classes\\SystemFileAssociations\\{}\\{}",
            ext, SHELL_KEY
        );

        // 创建 shell\zimufan 键并设置默认值（菜单显示文本）
        let (shell_key, _) = hkcu
            .create_subkey(&base)
            .map_err(|e| AppError::SystemContextMenuRegisterFailed {
                detail: format!("create key '{}': {}", base, e),
            })?;
        shell_key
            .set_value("", &menu_label)
            .map_err(|e| AppError::SystemContextMenuRegisterFailed {
                detail: format!("set default value '{}': {}", base, e),
            })?;

        // 创建 command 子键并设置执行命令
        let command_subkey = format!("{}\\command", base);
        let (cmd_key, _) = hkcu
            .create_subkey(&command_subkey)
            .map_err(|e| AppError::SystemContextMenuRegisterFailed {
                detail: format!("create key '{}': {}", command_subkey, e),
            })?;
        cmd_key
            .set_value("", &command_value)
            .map_err(|e| AppError::SystemContextMenuRegisterFailed {
                detail: format!("set command value '{}': {}", command_subkey, e),
            })?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn unregister_for_extensions(extensions: &[&str]) -> Result<(), AppError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    for ext in extensions {
        let shell_path = format!(
            "Software\\Classes\\SystemFileAssociations\\{}\\{}",
            ext, SHELL_KEY
        );
        // 删除 shell\zimufan 整个子树（含 command 子键）
        hkcu.delete_subkey_all(&shell_path).map_err(|e| {
            AppError::SystemContextMenuUnregisterFailed {
                detail: format!("delete key '{}': {}", shell_path, e),
            }
        })?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn is_registered_for_extensions(extensions: &[&str]) -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for ext in extensions {
        let shell_path = format!(
            "Software\\Classes\\SystemFileAssociations\\{}\\{}",
            ext, SHELL_KEY
        );
        if hkcu.open_subkey(&shell_path).is_err() {
            return false;
        }
    }
    true
}

// === SECTION 2 END ===

/// 注册视频文件右键菜单（Windows）
#[cfg(target_os = "windows")]
pub fn register_video_context_menu(exe_path: &str) -> Result<(), AppError> {
    register_for_extensions(VIDEO_EXTENSIONS, "AI-SubTrans 快速翻译", exe_path, "quick")
}

/// 注册字幕文件右键菜单（Windows）
#[cfg(target_os = "windows")]
pub fn register_subtitle_context_menu(exe_path: &str) -> Result<(), AppError> {
    register_for_extensions(SUBTITLE_EXTENSIONS, "AI-SubTrans 编辑字幕", exe_path, "edit")
}

/// 注销视频文件右键菜单（Windows）
#[cfg(target_os = "windows")]
pub fn unregister_video_context_menu() -> Result<(), AppError> {
    unregister_for_extensions(VIDEO_EXTENSIONS)
}

/// 注销字幕文件右键菜单（Windows）
#[cfg(target_os = "windows")]
pub fn unregister_subtitle_context_menu() -> Result<(), AppError> {
    unregister_for_extensions(SUBTITLE_EXTENSIONS)
}

/// 检查视频右键菜单是否已注册（Windows）
#[cfg(target_os = "windows")]
pub fn is_video_context_menu_registered() -> bool {
    is_registered_for_extensions(VIDEO_EXTENSIONS)
}

/// 检查字幕右键菜单是否已注册（Windows）
#[cfg(target_os = "windows")]
pub fn is_subtitle_context_menu_registered() -> bool {
    is_registered_for_extensions(SUBTITLE_EXTENSIONS)
}

/// 注册文件夹右键菜单「AI-SubTrans 批量翻译」（Windows）
/// 在文件夹上右键 → 添加该文件夹到批量翻译监视
/// 注册到两个位置：
///   1. Directory\shell（右键具体文件夹，%1 传入文件夹路径）
///   2. Directory\Background\shell（右键文件夹空白处，%V 传入当前目录路径）
#[cfg(target_os = "windows")]
pub fn register_folder_context_menu(exe_path: &str) -> Result<(), AppError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let menu_label = "AI-SubTrans 批量翻译";
    let icon_value = format!("\"{}\",0", exe_path);

    // 1. 注册到 Directory\shell（文件夹本身右键，%1 = 文件夹路径）
    let base_dir = "Software\\Classes\\Directory\\shell\\zimufan_batch";
    let (shell_key, _) = hkcu
        .create_subkey(base_dir)
        .map_err(|e| AppError::SystemContextMenuRegisterFailed {
            detail: format!("create key '{}': {}", base_dir, e),
        })?;
    shell_key
        .set_value("", &menu_label)
        .map_err(|e| AppError::SystemContextMenuRegisterFailed {
            detail: format!("set default value '{}': {}", base_dir, e),
        })?;
    shell_key
        .set_value("Icon", &icon_value)
        .map_err(|e| AppError::SystemContextMenuRegisterFailed {
            detail: format!("set Icon '{}': {}", base_dir, e),
        })?;

    let command_dir = format!("{}\\command", base_dir);
    let command_value_dir = format!("\"{}\" --mode=watch \"%1\"", exe_path);
    let (cmd_key, _) = hkcu
        .create_subkey(&command_dir)
        .map_err(|e| AppError::SystemContextMenuRegisterFailed {
            detail: format!("create key '{}': {}", command_dir, e),
        })?;
    cmd_key
        .set_value("", &command_value_dir)
        .map_err(|e| AppError::SystemContextMenuRegisterFailed {
            detail: format!("set command value '{}': {}", command_dir, e),
        })?;

    // 2. 注册到 Directory\Background\shell（右键文件夹空白处，%V = 当前目录路径）
    let base_bg = "Software\\Classes\\Directory\\Background\\shell\\zimufan_batch";
    let (bg_key, _) = hkcu
        .create_subkey(base_bg)
        .map_err(|e| AppError::SystemContextMenuRegisterFailed {
            detail: format!("create key '{}': {}", base_bg, e),
        })?;
    bg_key
        .set_value("", &menu_label)
        .map_err(|e| AppError::SystemContextMenuRegisterFailed {
            detail: format!("set default value '{}': {}", base_bg, e),
        })?;
    bg_key
        .set_value("Icon", &icon_value)
        .map_err(|e| AppError::SystemContextMenuRegisterFailed {
            detail: format!("set Icon '{}': {}", base_bg, e),
        })?;

    let command_bg = format!("{}\\command", base_bg);
    let command_value_bg = format!("\"{}\" --mode=watch \"%V\"", exe_path);
    let (cmd_bg_key, _) = hkcu
        .create_subkey(&command_bg)
        .map_err(|e| AppError::SystemContextMenuRegisterFailed {
            detail: format!("create key '{}': {}", command_bg, e),
        })?;
    cmd_bg_key
        .set_value("", &command_value_bg)
        .map_err(|e| AppError::SystemContextMenuRegisterFailed {
            detail: format!("set command value '{}': {}", command_bg, e),
        })?;

    Ok(())
}

/// 注销文件夹右键菜单（Windows）
#[cfg(target_os = "windows")]
pub fn unregister_folder_context_menu() -> Result<(), AppError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    // 删除 Directory\shell\zimufan_batch
    let base_dir = "Software\\Classes\\Directory\\shell\\zimufan_batch";
    let _ = hkcu.delete_subkey_all(base_dir);
    // 删除 Directory\Background\shell\zimufan_batch
    let base_bg = "Software\\Classes\\Directory\\Background\\shell\\zimufan_batch";
    hkcu.delete_subkey_all(base_bg).map_err(|e| {
        AppError::SystemContextMenuUnregisterFailed {
            detail: format!("delete key '{}': {}", base_bg, e),
        }
    })
}

/// 检查文件夹右键菜单是否已注册（Windows）
#[cfg(target_os = "windows")]
pub fn is_folder_context_menu_registered() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let base = "Software\\Classes\\Directory\\shell\\zimufan_batch";
    hkcu.open_subkey(base).is_ok()
}

// === SECTION 3 END ===

// ===== 非 Windows 平台的 stub 实现 =====

#[cfg(not(target_os = "windows"))]
pub fn register_video_context_menu(_exe_path: &str) -> Result<(), AppError> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn register_subtitle_context_menu(_exe_path: &str) -> Result<(), AppError> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn unregister_video_context_menu() -> Result<(), AppError> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn unregister_subtitle_context_menu() -> Result<(), AppError> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn is_video_context_menu_registered() -> bool {
    false
}

#[cfg(not(target_os = "windows"))]
pub fn is_subtitle_context_menu_registered() -> bool {
    false
}

#[cfg(not(target_os = "windows"))]
pub fn register_folder_context_menu(_exe_path: &str) -> Result<(), AppError> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn unregister_folder_context_menu() -> Result<(), AppError> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn is_folder_context_menu_registered() -> bool {
    false
}

// === SECTION 4 END ===

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn video_extensions_contains_expected_formats() {
        assert!(VIDEO_EXTENSIONS.contains(&".mkv"));
        assert!(VIDEO_EXTENSIONS.contains(&".mp4"));
        assert!(VIDEO_EXTENSIONS.contains(&".avi"));
        assert!(VIDEO_EXTENSIONS.contains(&".mov"));
        assert!(VIDEO_EXTENSIONS.contains(&".wmv"));
        assert!(VIDEO_EXTENSIONS.contains(&".flv"));
        assert!(VIDEO_EXTENSIONS.contains(&".ts"));
        assert!(VIDEO_EXTENSIONS.contains(&".m2ts"));
    }

    #[test]
    fn video_extensions_count() {
        assert_eq!(VIDEO_EXTENSIONS.len(), 8);
    }

    #[test]
    fn subtitle_extensions_contains_expected_formats() {
        assert!(SUBTITLE_EXTENSIONS.contains(&".srt"));
        assert!(SUBTITLE_EXTENSIONS.contains(&".ass"));
        assert!(SUBTITLE_EXTENSIONS.contains(&".ssa"));
        assert!(SUBTITLE_EXTENSIONS.contains(&".vtt"));
        assert!(SUBTITLE_EXTENSIONS.contains(&".sub"));
    }

    #[test]
    fn subtitle_extensions_count() {
        assert_eq!(SUBTITLE_EXTENSIONS.len(), 5);
    }

    #[test]
    fn video_and_subtitle_extensions_disjoint() {
        for ext in VIDEO_EXTENSIONS {
            assert!(
                !SUBTITLE_EXTENSIONS.contains(ext),
                "{} 不应同时出现在视频和字幕扩展名列表中",
                ext
            );
        }
    }

    #[test]
    fn video_extensions_all_start_with_dot() {
        for ext in VIDEO_EXTENSIONS {
            assert!(ext.starts_with('.'), "扩展名 {} 应以 '.' 开头", ext);
        }
    }

    #[test]
    fn subtitle_extensions_all_start_with_dot() {
        for ext in SUBTITLE_EXTENSIONS {
            assert!(ext.starts_with('.'), "扩展名 {} 应以 '.' 开头", ext);
        }
    }

    #[test]
    fn shell_key_value() {
        assert_eq!(SHELL_KEY, "shell\\zimufan");
    }
}

// === SECTION FINAL END ===
