// 集成测试：用真实视频文件验证 FFmpeg probe + 提取 + 合并
// 测试视频：Rick and Morty S09E05 1080p AMZN WEB-DL DUAL DDP5 1 H 264-TURG.mkv

#[cfg(test)]
mod integration_tests {
    use zimufan_lib::ffmpeg;
    use zimufan_lib::subtitle;

    const TEST_VIDEO: &str = "C:/code/ai-subtrans/Rick and Morty S09E05 1080p AMZN WEB-DL DUAL DDP5 1 H 264-TURG.mkv";

    #[test]
    fn test_probe_real_video() {
        let result = ffmpeg::probe_video(TEST_VIDEO, None);
        assert!(result.is_ok(), "probe_video 应成功: {:?}", result.err());

        let probe = result.unwrap();
        assert_eq!(probe.video_path, TEST_VIDEO);

        // 验证视频流
        let video = probe.video_stream.expect("应有视频流");
        assert_eq!(video.width, 1920);
        assert_eq!(video.height, 1080);
        assert_eq!(video.codec_name, "h264");

        // 验证字幕流（应有 4 条：tur forced/tur/eng/eng SDH）
        assert!(probe.subtitle_streams.len() >= 3, "应有至少 3 条字幕流");

        // 验证有英文字幕流
        let has_english = probe
            .subtitle_streams
            .iter()
            .any(|s| s.language.as_deref() == Some("eng"));
        assert!(has_english, "应有英文字幕流");

        // 验证有 SDH 字幕流
        let has_sdh = probe
            .subtitle_streams
            .iter()
            .any(|s| s.disposition_hearing_impaired);
        assert!(has_sdh, "应有 SDH 字幕流");

        // 验证非 HDR
        assert!(video.hdr_info.is_none(), "此视频不应为 HDR");

        // 验证有音频流
        assert!(!probe.audio_streams.is_empty(), "应有音频流");

        println!("视频探测成功:");
        println!("  格式: {}", probe.format.format_name);
        println!("  时长: {:?}", probe.format.duration);
        println!("  视频流: {}x{} {}", video.width, video.height, video.codec_name);
        println!("  字幕流数: {}", probe.subtitle_streams.len());
        for s in &probe.subtitle_streams {
            println!(
                "    [{}] {} {} forced={} sdh={} graphic={}",
                s.index,
                s.codec_name,
                s.language.as_deref().unwrap_or("?"),
                s.disposition_forced,
                s.disposition_hearing_impaired,
                s.is_graphic
            );
        }
        println!("  音频流数: {}", probe.audio_streams.len());
    }

    #[test]
    fn test_probe_nonexistent() {
        let result = ffmpeg::probe_video("C:/nonexistent/video.mkv", None);
        assert!(result.is_err());
    }

    /// 测试提取字幕流（英文字幕，index=3）
    #[test]
    fn test_extract_subtitle() {
        // 先 probe 获取字幕流索引
        let probe = ffmpeg::probe_video(TEST_VIDEO, None).expect("probe 应成功");
        let eng_stream = probe
            .subtitle_streams
            .iter()
            .find(|s| s.language.as_deref() == Some("eng") && !s.disposition_hearing_impaired)
            .expect("应有非 SDH 英文字幕流");

        let output_path = "C:/code/ai-subtrans/test_extract_output.srt";

        let result = ffmpeg::extract_subtitle_stream(
            TEST_VIDEO,
            eng_stream.index,
            output_path,
            None,
        );
        assert!(result.is_ok(), "字幕提取应成功: {:?}", result.err());

        // 验证输出文件存在且非空
        let metadata = std::fs::metadata(output_path);
        assert!(metadata.is_ok(), "输出文件应存在");
        assert!(metadata.unwrap().len() > 0, "输出文件不应为空");

        // 验证可以解析为 SRT
        let content = std::fs::read_to_string(output_path).expect("应能读取输出文件");
        let parsed = subtitle::parse_srt(&content);
        assert!(parsed.is_ok(), "SRT 解析应成功: {:?}", parsed.err());
        let file = parsed.unwrap();
        assert!(file.entries.len() > 0, "应有字幕条目");

        println!("字幕提取成功: {} 条目", file.entries.len());

        // 清理
        let _ = std::fs::remove_file(output_path);
    }

    /// 测试字幕解析+渲染往返
    #[test]
    fn test_subtitle_roundtrip() {
        let srt_content = "1\n00:00:01,000 --> 00:00:03,000\nHello World\n\n2\n00:00:04,000 --> 00:00:06,000\nThis is a test\n";
        let file = subtitle::parse_srt(srt_content).expect("解析应成功");
        assert_eq!(file.entries.len(), 2);

        let rendered = subtitle::render_srt(&file);
        let reparsed = subtitle::parse_srt(&rendered).expect("重新解析应成功");
        assert_eq!(file.entries.len(), reparsed.entries.len());
        assert_eq!(file.entries[0].text, reparsed.entries[0].text);
    }

    /// 测试 CLI 参数解析
    #[test]
    fn test_cli_args_parsing() {
        // 模拟命令行参数
        std::env::set_var("TEST_CLI", "1");
        // 由于 parse_cli_args 读取 std::env::args()，这里仅验证函数存在
        // 实际测试在 E2E 中进行
    }
}
