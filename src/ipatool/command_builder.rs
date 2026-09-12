//! ipatool 命令行参数构建。
//!
//! 移植自主仓库 `IpatoolCommandBuilder`：所有命令统一追加 `--format json`、
//! `--non-interactive`、`--verbose`；除 `auth revoke` 外追加 `--keychain-passphrase`。
//! 加密密钥由宿主解析后传入，Core 不读取任何配置。

/// 判断参数列表是否为退出登录命令（`auth revoke`，大小写不敏感）。
pub fn is_logout(args: &[impl AsRef<str>]) -> bool {
    args.len() >= 2
        && args[0].as_ref().eq_ignore_ascii_case("auth")
        && args[1].as_ref().eq_ignore_ascii_case("revoke")
}

/// 构建标准命令参数：`is_logout` 为 true 时省略 `--keychain-passphrase`。
pub fn build_standard_arguments(
    arguments: &[impl AsRef<str>],
    passphrase: &str,
    is_logout: bool,
) -> Vec<String> {
    let mut final_arguments: Vec<String> = arguments
        .iter()
        .map(|argument| argument.as_ref().to_string())
        .collect();
    if !is_logout {
        final_arguments.push("--keychain-passphrase".to_string());
        final_arguments.push(passphrase.to_string());
    }
    final_arguments.extend(
        ["--format", "json", "--non-interactive", "--verbose"]
            .iter()
            .copied()
            .map(str::to_string),
    );
    final_arguments
}

/// 构建下载命令参数（含 `--purchase`，下载即购买）。
pub fn build_download_arguments(
    bundle_id: &str,
    output_directory: &str,
    passphrase: &str,
) -> Vec<String> {
    [
        "download",
        "--output",
        output_directory,
        "--bundle-identifier",
        bundle_id,
        "--purchase",
        "--keychain-passphrase",
        passphrase,
        "--format",
        "json",
        "--non-interactive",
        "--verbose",
    ]
    .iter()
    .copied()
    .map(str::to_string)
    .collect()
}

/// 构建 list-purchases 命令参数；单页数量受 ipatool 限制不得超过 100。
pub fn build_list_purchases_arguments(max_results: i64, page: i64) -> Vec<String> {
    [
        "list-purchases",
        "--max-results",
        &max_results.to_string(),
        "--page",
        &page.to_string(),
    ]
    .iter()
    .copied()
    .map(str::to_string)
    .collect()
}

/// 子进程环境变量：禁用彩色输出与交互式终端。
pub fn create_environment_variables() -> Vec<(&'static str, &'static str)> {
    vec![("NO_COLOR", "1"), ("TERM", "dumb")]
}

/// 安全命令标签：最多取前两个参数，用于超时等错误提示，不泄露密钥等敏感值。
pub fn get_safe_command_label(arguments: &[impl AsRef<str>]) -> String {
    match arguments.len() {
        0 => "ipatool".to_string(),
        1 => format!("ipatool {}", arguments[0].as_ref()),
        _ => format!(
            "ipatool {} {}",
            arguments[0].as_ref(),
            arguments[1].as_ref()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_logout_recognizes_only_auth_revoke() {
        let cases = [
            (vec!["auth", "revoke"], true),
            (vec!["AUTH", "REVOKE"], true),
            (vec!["auth", "login"], false),
            (vec!["purchase", "revoke"], false),
        ];

        for (arguments, expected) in cases {
            assert_eq!(is_logout(&arguments), expected);
        }
    }

    #[test]
    fn is_logout_rejects_short_argument_lists() {
        let empty: Vec<&str> = Vec::new();
        assert!(!is_logout(&empty));
        assert!(!is_logout(&["revoke"]));
    }

    #[test]
    fn build_standard_arguments_appends_passphrase_and_global_switches() {
        let arguments = build_standard_arguments(&["auth", "info"], "secret", false);

        assert_eq!(
            arguments,
            [
                "auth",
                "info",
                "--keychain-passphrase",
                "secret",
                "--format",
                "json",
                "--non-interactive",
                "--verbose"
            ]
        );
    }

    #[test]
    fn build_standard_arguments_omits_passphrase_for_logout() {
        let arguments = build_standard_arguments(&["auth", "revoke"], "secret", true);

        assert_eq!(
            arguments,
            [
                "auth",
                "revoke",
                "--format",
                "json",
                "--non-interactive",
                "--verbose"
            ]
        );
    }

    #[test]
    fn build_download_arguments_builds_complete_command() {
        let arguments = build_download_arguments("com.example.app", "C:\\Downloads", "secret");

        assert_eq!(
            arguments,
            [
                "download",
                "--output",
                "C:\\Downloads",
                "--bundle-identifier",
                "com.example.app",
                "--purchase",
                "--keychain-passphrase",
                "secret",
                "--format",
                "json",
                "--non-interactive",
                "--verbose"
            ]
        );
    }

    #[test]
    fn build_list_purchases_arguments_includes_paging() {
        assert_eq!(
            build_list_purchases_arguments(100, 3),
            ["list-purchases", "--max-results", "100", "--page", "3"]
        );
    }

    #[test]
    fn create_environment_variables_disables_color_and_interactive_term() {
        let variables = create_environment_variables();

        assert_eq!(variables.len(), 2);
        assert!(variables.contains(&("NO_COLOR", "1")));
        assert!(variables.contains(&("TERM", "dumb")));
    }

    #[test]
    fn get_safe_command_label_uses_at_most_two_arguments() {
        let cases: [(Vec<&str>, &str); 4] = [
            (Vec::new(), "ipatool"),
            (vec!["auth"], "ipatool auth"),
            (vec!["auth", "info"], "ipatool auth info"),
            (vec!["auth", "info", "--verbose"], "ipatool auth info"),
        ];

        for (arguments, expected) in cases {
            assert_eq!(get_safe_command_label(&arguments), expected);
        }
    }
}
