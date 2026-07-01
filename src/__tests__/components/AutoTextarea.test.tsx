// AutoTextarea 组件测试
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AutoTextarea } from "../../components/AutoTextarea";

// === SECTION 1 END ===

describe("AutoTextarea - 渲染与交互", () => {
  it("渲染初始值", () => {
    render(<AutoTextarea value="hello" onChange={() => {}} />);
    expect(screen.getByDisplayValue("hello")).toBeInTheDocument();
  });

  it("渲染 placeholder", () => {
    render(<AutoTextarea value="" onChange={() => {}} placeholder="输入文本" />);
    expect(screen.getByPlaceholderText("输入文本")).toBeInTheDocument();
  });

  it("输入触发 onChange", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<AutoTextarea value="" onChange={onChange} />);
    const textarea = screen.getByRole("textbox");
    await user.type(textarea, "a");
    expect(onChange).toHaveBeenCalled();
  });

  it("点击触发 onClick", () => {
    const onClick = vi.fn();
    render(<AutoTextarea value="" onChange={() => {}} onClick={onClick} />);
    fireEvent.click(screen.getByRole("textbox"));
    expect(onClick).toHaveBeenCalled();
  });

  it("键盘事件触发 onKeyDown", () => {
    const onKeyDown = vi.fn();
    render(<AutoTextarea value="" onChange={() => {}} onKeyDown={onKeyDown} />);
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    expect(onKeyDown).toHaveBeenCalled();
  });

  it("右键菜单触发 onContextMenu", () => {
    const onContextMenu = vi.fn();
    render(<AutoTextarea value="" onChange={() => {}} onContextMenu={onContextMenu} />);
    fireEvent.contextMenu(screen.getByRole("textbox"));
    expect(onContextMenu).toHaveBeenCalled();
  });

  it("autoFocus 自动聚焦", () => {
    render(<AutoTextarea value="" onChange={() => {}} autoFocus />);
    const textarea = screen.getByRole("textbox");
    expect(document.activeElement).toBe(textarea);
  });
});

// === SECTION 2 END ===
