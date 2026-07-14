import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { RestoreOriginalDialog } from "../../components/RestoreOriginalDialog";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, fallback: string) => fallback,
  }),
}));

vi.mock("../../components/ui/dialog", () => ({
  Dialog: ({ children, open }: any) => open ? <div data-testid="dialog">{children}</div> : null,
  DialogContent: ({ children }: any) => <div>{children}</div>,
  DialogHeader: ({ children }: any) => <div>{children}</div>,
  DialogTitle: ({ children }: any) => <h2>{children}</h2>,
  DialogDescription: ({ children }: any) => <p>{children}</p>,
}));

vi.mock("../../components/ui/button", () => ({
  Button: ({ children, onClick, variant }: any) => (
    <button data-variant={variant} onClick={onClick}>{children}</button>
  ),
}));

describe("RestoreOriginalDialog", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("open=false 时不渲染", () => {
    const { container } = render(
      <RestoreOriginalDialog open={false} onOpenChange={() => {}} originalText="Hello" modifiedText="Hi" onRestore={() => {}} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("open=true 时显示原始文本和修改后文本", () => {
    render(
      <RestoreOriginalDialog open={true} onOpenChange={() => {}} originalText="Hello" modifiedText="Hi" onRestore={() => {}} />,
    );
    expect(screen.getByText("Hello")).toBeInTheDocument();
    expect(screen.getByText("Hi")).toBeInTheDocument();
  });

  it("点击恢复按钮调用 onRestore + onOpenChange(false)", () => {
    const onRestore = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <RestoreOriginalDialog open={true} onOpenChange={onOpenChange} originalText="Hello" modifiedText="Hi" onRestore={onRestore} />,
    );
    const buttons = screen.getAllByRole("button");
    // 最后一个按钮是"恢复"
    fireEvent.click(buttons[buttons.length - 1]);
    expect(onRestore).toHaveBeenCalledTimes(1);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("点击取消按钮调用 onOpenChange(false) 不调 onRestore", () => {
    const onRestore = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <RestoreOriginalDialog open={true} onOpenChange={onOpenChange} originalText="Hello" modifiedText="Hi" onRestore={onRestore} />,
    );
    const buttons = screen.getAllByRole("button");
    // 第一个按钮是"取消"
    fireEvent.click(buttons[0]);
    expect(onRestore).not.toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
