import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DependencyDialog, type DependencyInfo } from "./DependencyDialog";

const deps: DependencyInfo[] = [
  { projectId: "a", title: "Vault", versionName: "1.7", required: true, alreadyInstalled: false, fileName: "vault.jar" },
  { projectId: "b", title: "PlaceholderAPI", versionName: "2.11", required: false, alreadyInstalled: false, fileName: "papi.jar" },
  { projectId: "c", title: "LuckPerms", versionName: "5.4", required: false, alreadyInstalled: true, fileName: "lp.jar" },
];

describe("DependencyDialog", () => {
  it("splits cards into required, optional and already-installed", () => {
    render(<DependencyDialog title="Foo" dependencies={deps} chosen={["a"]} busy={false} onToggle={() => {}} onCancel={() => {}} onConfirm={() => {}} />);
    expect(document.querySelectorAll(".dep-card.fixed")).toHaveLength(1);
    expect(document.querySelectorAll(".dep-card.pick")).toHaveLength(1);
    expect(document.querySelectorAll(".dep-card.done")).toHaveLength(1);
    expect(screen.getByRole("button", { name: /Install 2/ })).toBeTruthy();
  });

  it("picking an optional card fills it and counts it", () => {
    const onToggle = vi.fn();
    const { rerender } = render(<DependencyDialog title="Foo" dependencies={deps} chosen={["a"]} busy={false} onToggle={onToggle} onCancel={() => {}} onConfirm={() => {}} />);
    fireEvent.click(document.querySelector(".dep-card.pick")!);
    expect(onToggle).toHaveBeenCalledWith("b");
    rerender(<DependencyDialog title="Foo" dependencies={deps} chosen={["a", "b"]} busy={false} onToggle={onToggle} onCancel={() => {}} onConfirm={() => {}} />);
    expect(document.querySelector(".dep-card.pick")!.className).toContain("on");
    expect(screen.getByRole("button", { name: /Install 3/ })).toBeTruthy();
  });

  it("closes with the X and never lists a text close button", () => {
    const onCancel = vi.fn();
    render(<DependencyDialog title="Foo" dependencies={deps} chosen={["a"]} busy={false} onToggle={() => {}} onCancel={onCancel} onConfirm={() => {}} />);
    fireEvent.click(document.querySelector(".dep-dialog .close")!);
    expect(onCancel).toHaveBeenCalled();
    expect(document.querySelector(".dep-dialog .modal-actions .button.ghost")).toBeNull();
  });
});
