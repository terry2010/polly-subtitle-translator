// HdrNotice 组件测试
import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { HdrNotice } from "../../components/HdrNotice";
import { useVideoStore } from "../../stores/videoStore";
import type { ProbeResult } from "../../lib/ipc-types";

function makeProbe(hdrInfo: any): ProbeResult {
  return {
    video_path: "/test.mkv",
    format: { format_name: "matroska", format_long_name: "Matroska", duration: 120, size: 1000, bit_rate: 8000 },
    video_stream: { hdr_info: hdrInfo } as any,
    audio_streams: [],
    subtitle_streams: [],
  };
}

beforeEach(() => {
  useVideoStore.setState({ probeResult: null, loading: false, error: null, selectedSubtitleStream: null });
});

// === SECTION 1 END ===

describe("HdrNotice - 渲染", () => {
  it("无 probeResult 时不渲染", () => {
    const { container } = render(<HdrNotice />);
    expect(container.firstChild).toBeNull();
  });

  it("无 hdr_info 时不渲染", () => {
    useVideoStore.setState({ probeResult: makeProbe(null) });
    const { container } = render(<HdrNotice />);
    expect(container.firstChild).toBeNull();
  });

  it("有 HDR 信息时渲染提示", () => {
    useVideoStore.setState({
      probeResult: makeProbe({ hdr_format: "HDR10", details: "BT.2020 PQ", is_dolby_vision: false }),
    });
    render(<HdrNotice />);
    expect(screen.getByText("HDR10")).toBeInTheDocument();
    expect(screen.getByText("BT.2020 PQ")).toBeInTheDocument();
  });

  it("Dolby Vision 显示额外提示", () => {
    useVideoStore.setState({
      probeResult: makeProbe({ hdr_format: "Dolby Vision", details: "", is_dolby_vision: true }),
    });
    render(<HdrNotice />);
    expect(screen.getByText(/Dolby Vision 内容可能需要兼容播放器/)).toBeInTheDocument();
  });
});

// === SECTION 2 END ===

describe("HdrNotice - 关闭", () => {
  it("点击关闭按钮隐藏提示", () => {
    useVideoStore.setState({
      probeResult: makeProbe({ hdr_format: "HDR10", details: "", is_dolby_vision: false }),
    });
    render(<HdrNotice />);
    const closeBtn = screen.getByRole("button");
    fireEvent.click(closeBtn);
    expect(screen.queryByText("HDR10")).not.toBeInTheDocument();
  });

  it("切换视频后重置 dismissed 状态", () => {
    useVideoStore.setState({
      probeResult: makeProbe({ hdr_format: "HDR10", details: "", is_dolby_vision: false }),
    });
    const { rerender } = render(<HdrNotice />);
    fireEvent.click(screen.getByRole("button"));
    expect(screen.queryByText("HDR10")).not.toBeInTheDocument();

    // 切换到新视频
    act(() => {
      useVideoStore.setState({
        probeResult: makeProbe({ hdr_format: "HDR10+", details: "", is_dolby_vision: false }),
      });
    });
    rerender(<HdrNotice />);
    expect(screen.getByText("HDR10+")).toBeInTheDocument();
  });
});

// === SECTION 3 END ===
